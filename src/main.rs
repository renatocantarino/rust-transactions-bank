use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::sync::RwLock;

#[derive(Default, Clone, Serialize)]
struct Account {
    #[serde(rename = "total")]
    balance: i64,

    #[serde(rename = "limite")]
    limit: i64,

    transactions: RingBuffer<Transaction>,
}

#[derive(Clone, Serialize)]
struct RingBuffer<Transaction>(VecDeque<Transaction>);

impl<Transaction> RingBuffer<Transaction> {
    fn new(capacity: usize) -> Self {
        Self(VecDeque::with_capacity(capacity))
    }
    fn push(&mut self, item: Transaction) {
        if self.0.len() == self.0.capacity() {
            self.0.pop_back();
            self.0.push_front(item)
        } else {
            self.0.push_front(item)
        }
    }
}

impl Default for RingBuffer<Transaction> {
    fn default() -> Self {
        Self::new(10)
    }
}

impl Account {
    pub fn with_limit(limit: i64) -> Self {
        Account {
            limit,
            ..Default::default()
        }
    }

    pub fn transact(&mut self, transaction: Transaction) -> Result<(), &'static str> {
        match transaction.kind {
            TransactionType::CREDIT => {
                self.balance += transaction.value;
                self.transactions.push(transaction);
                Ok(())
            }
            TransactionType::DEBIT => {
                if self.limit + self.balance >= transaction.value {
                    self.balance -= transaction.value;
                    self.transactions.push(transaction);
                    Ok(())
                } else {
                    Err("Limite insuficiente")
                }
            }
        }
    }
}

type AppState = Arc<HashMap<u8, RwLock<Account>>>;

#[derive(Clone, Serialize, Deserialize)]
enum TransactionType {
    #[serde(rename = "C")]
    CREDIT,

    #[serde(rename = "D")]
    DEBIT,
}

#[derive(Clone, Serialize, Deserialize)]
struct Transaction {
    #[serde(rename = "valor")]
    value: i64,

    #[serde(rename = "tipo")]
    kind: TransactionType,

    #[serde(rename = "descricao")]
    description: Description,

    #[serde(
        rename = "realizada_em",
        with = "time::serde::rfc3339",
        default = "OffsetDateTime::now_utc"
    )]
    create_at: OffsetDateTime,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(try_from = "String")]
struct Description(String);

impl TryFrom<String> for Description {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() || value.len() > 10 {
            Err("Descrição invalida")
        } else {
            Ok(Self(value))
        }
    }
}

#[tokio::main]
async fn main() {
    let account_in_memory = HashMap::<u8, RwLock<Account>>::from_iter([
        (1, RwLock::new(Account::with_limit(100_000))),
        (2, RwLock::new(Account::with_limit(80_000))),
        (3, RwLock::new(Account::with_limit(1_000_000))),
        (4, RwLock::new(Account::with_limit(10_000_000))),
        (5, RwLock::new(Account::with_limit(500_000))),
    ]);

    let app = Router::new()
        .route("/", get(|| async { "Ola" }))
        .route("/clientes/:id/transacoes", post(create_transaction))
        .route("/clientes/:id/extrato", get(view_extrato))
        .with_state(Arc::new(account_in_memory));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn create_transaction(
    Path(account_id): Path<u8>,
    State(account_in_memory): State<AppState>,
    Json(transaction): Json<Transaction>,
) -> impl IntoResponse {
    match account_in_memory.get(&account_id) {
        Some(acc) => {
            let mut account = acc.write().await;
            match account.transact(transaction) {
                Ok(()) => Ok(Json(json!({
                    "account" : account_id,
                    "limite": account.limit,
                    "saldo": account.balance
                }))),
                Err(_) => Err(StatusCode::UNPROCESSABLE_ENTITY),
            }
        }
        None => Err(StatusCode::NOT_FOUND),
    }
}

async fn view_extrato(
    Path(account_id): Path<u8>,
    State(account_in_memory): State<AppState>,
) -> impl IntoResponse {
    match account_in_memory.get(&account_id) {
        Some(acc) => {
            let account = acc.read().await;
            Ok(Json(json!({
                "account" : account_id,
                "saldo": {
                    "total": account.balance,
                    "limite": account.limit,
                    "data_extrato": OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
                },
                "ultimas_transacoes": account.transactions

            })))
        }
        None => Err(StatusCode::NOT_FOUND),
    }
}
