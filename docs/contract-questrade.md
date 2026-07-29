# Questrade REST API Contract Specification (`contract-questrade.md`)

## 1. Overview & Authentication Flow

Questrade REST API uses **OAuth 2.0** protocol for authorization. Access tokens are short-lived (valid for 30 minutes / 1800 seconds) and must be rotated using the long-lived `refresh_token`.

> [!IMPORTANT]
> **Refresh Token Rotation**: Questrade rotates the `refresh_token` upon **every** exchange request. The API response returns a brand new `refresh_token` while invalidating the old one. If the new `refresh_token` is lost or fails to persist locally, the application loses access and requires manual user intervention via Questrade's API Centre.

### 1.1 Token Refresh Endpoint
* **Endpoint URL**: `https://login.questrade.com/oauth2/token`
* **HTTP Method**: `POST`
* **Query Parameters**:
  * `grant_type`: `"refresh_token"`
  * `refresh_token`: `<CURRENT_REFRESH_TOKEN>`

#### Token Response Payload
```json
{
  "access_token": "a1b2c3d4...",
  "token_type": "Bearer",
  "expires_in": 1800,
  "refresh_token": "r9876543...",
  "api_server": "https://api06.iq.questrade.com/"
}
```

### 1.2 Making Authenticated REST Requests
All API calls must include the returned `access_token` in the HTTP Authorization header as a Bearer token:
```http
Authorization: Bearer <access_token>
```
All REST API endpoints are hosted on the dynamically assigned `api_server` URL returned in the token response (e.g. `https://api06.iq.questrade.com/v1/...`).

---

## 2. Core API Endpoints Contract

### 2.1 Accounts Endpoint (`GET /v1/accounts`)
Retrieves the list of accounts associated with the authorized user profile.

> **Hi5bot usage**: Called at startup by `src/accounts.rs::discover()` to dynamically discover active accounts matching the configured `account_types` whitelist. Replaces the old hardcoded `resp_account`/`tfsa_account` fields.

* **URL Path**: `GET {api_server}/v1/accounts`
* **Response Payload**:
```json
{
  "accounts": [
    {
      "type": "Margin",
      "number": "51638291",
      "status": "Active",
      "isPrimary": true,
      "isBilling": true,
      "clientAccountType": "Individual"
    }
  ]
}
```

---

### 2.2 Account Balances Endpoint (`GET /v1/accounts/{accountId}/balances`)
Retrieves cash balances, equity, and buying power for a specific account.

* **URL Path**: `GET {api_server}/v1/accounts/{accountId}/balances`
* **Response Payload**:
```json
{
  "perCurrencyBalances": [
    {
      "currency": "USD",
      "cash": 5420.50,
      "marketValue": 24850.00,
      "totalEquity": 30270.50,
      "buyingPower": 5420.50
    },
    {
      "currency": "CAD",
      "cash": 120.00,
      "marketValue": 0.00,
      "totalEquity": 120.00,
      "buyingPower": 120.00
    }
  ],
  "combinedBalances": [
    {
      "currency": "CAD",
      "cash": 7470.67,
      "marketValue": 33547.50,
      "totalEquity": 41018.17,
      "buyingPower": 7470.67
    }
  ]
}
```

> [!CAUTION]
> **USD Hard Lock Rule**: Hi5bot strictly parses `perCurrencyBalances` where `currency == "USD"`. The system enforces that `cash` must cover the entire order amount before submitting any trade to prevent CAD margin borrowing fees.

---

### 2.3 Account Positions Endpoint (`GET /v1/accounts/{accountId}/positions`)
Retrieves active security positions for an account.

* **URL Path**: `GET {api_server}/v1/accounts/{accountId}/positions`
* **Response Payload**:
```json
{
  "positions": [
    {
      "symbol": "IWY",
      "symbolId": 328491,
      "openQuantity": 15,
      "closedQuantity": 0,
      "currentMarketValue": 2550.00,
      "currentPrice": 170.00,
      "averageEntryPrice": 162.50
    },
    {
      "symbol": "SPMO",
      "symbolId": 419204,
      "openQuantity": 25,
      "closedQuantity": 0,
      "currentMarketValue": 2400.00,
      "currentPrice": 96.00,
      "averageEntryPrice": 91.20
    }
  ]
}
```

---

### 2.4 Market Candles Endpoint (`GET /v1/markets/candles/{symbolId}`)
Retrieves historical market candlestick prices for a given symbol identifier.

* **URL Path**: `GET {api_server}/v1/markets/candles/{symbolId}`
* **Query Parameters**:
  * `startTime`: ISO 8601 string (e.g. `2026-07-01T00:00:00-04:00`)
  * `endTime`: ISO 8601 string (e.g. `2026-07-29T15:30:00-04:00`)
  * `interval`: Granularity, e.g. `"OneDay"`, `"FiveMinutes"`
* **Response Payload**:
```json
{
  "candles": [
    {
      "start": "2026-07-28T00:00:00.000000-04:00",
      "end": "2026-07-29T00:00:00.000000-04:00",
      "low": 94.50,
      "high": 96.80,
      "open": 95.10,
      "close": 96.00,
      "volume": 1245000
    }
  ]
}
```

---

### 2.5 Order Execution Endpoint (`POST /v1/accounts/{accountId}/orders`)
Submits an order for execution on Questrade.

* **URL Path**: `POST {api_server}/v1/accounts/{accountId}/orders`
* **Request Body Payload**:
```json
{
  "symbolId": 328491,
  "quantity": 5,
  "limitPrice": 170.00,
  "action": "Buy",
  "orderType": "Limit",
  "timeInForce": "Day",
  "primaryRoute": "AUTO"
}
```
* **Response Payload**:
```json
{
  "orderId": 98472910,
  "orders": [
    {
      "id": 98472910,
      "symbol": "IWY",
      "symbolId": 328491,
      "totalQuantity": 5,
      "openQuantity": 5,
      "filledQuantity": 0,
      "action": "Buy",
      "limitPrice": 170.00,
      "orderType": "Limit",
      "state": "Accepted"
    }
  ]
}
```

---

## 3. Error Codes & Handling Contract

Questrade REST API returns standard HTTP status codes along with custom JSON error responses.

### 3.1 Common HTTP Status & Error Codes

| HTTP Status | Error Code | Description | Handling Protocol |
| :--- | :--- | :--- | :--- |
| `401 Unauthorized` | `1004` / `1013` | Invalid or expired Access Token | Trigger `ensure_valid()` to refresh OAuth token |
| `400 Bad Request` | `1002` | Invalid Request Parameters / Order Rejection | Log error payload and trigger notification alert |
| `429 Too Many Requests` | `1006` | Rate Limit Exceeded (Limit: ~30 req/min) | Exponential backoff retry |
| `500 Server Error` | `1000` | Questrade Gateway Error | Retry up to 3 times before tripping circuit breaker |

### 3.2 Error Payload Schema
```json
{
  "code": 1004,
  "message": "Access token has expired."
}
```

---

## 4. Rust Integration Strategy (`src/auth.rs` & `src/engine.rs`)

1. **Atomic File Persistence (`tokens.json`)**: Uses `save_atomic` with Unix `0600` permissions and `.bak` fallback copy before every write.
2. **Double-Checked Expiry Lock**: Cheap in-memory mutex check followed by an async `refresh_lock` guard to serialize concurrent requests.
3. **Safe Parsing**: Decouples Questrade camelCase fields using `serde(rename_all = "camelCase")` and maps financial numbers to `rust_decimal::Decimal`.
