//! CORS, JWT authorization and the `AUTH_AT_FIRST` prompt.

use super::*;

/// Hono's `cors()` defaults plus `Access-Control-Allow-Private-Network`
/// (for web remote controls).
pub(crate) async fn cors(request: Request, next: Next) -> Response {
    let mut response = if request.method() == Method::OPTIONS {
        let mut response = StatusCode::NO_CONTENT.into_response();
        let headers = response.headers_mut();
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET,HEAD,PUT,POST,DELETE,PATCH"),
        );
        if let Some(requested) = request.headers().get(header::ACCESS_CONTROL_REQUEST_HEADERS) {
            headers.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, requested.clone());
            headers.insert(header::VARY, HeaderValue::from_static("Access-Control-Request-Headers"));
        }
        response
    } else {
        next.run(request).await
    };
    let headers = response.headers_mut();
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    headers.insert("access-control-allow-private-network", HeaderValue::from_static("true"));
    response
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, "Unauthorized").into_response()
}

/// Whether `token` belongs to an authorized client (or auth is off).
pub(super) fn authorized(config: &Value, token: Option<&str>) -> bool {
    if config["authStrategy"] == NONE {
        return true;
    }
    let secret = config["secret"].as_str().unwrap_or_default();
    let Some(payload) = token.and_then(|token| jwt::verify(token, secret)) else { return false };
    let clients = config["authorizedClients"].as_array();
    payload["id"].as_str().is_some_and(|id| clients.is_some_and(|c| c.iter().any(|c| c == id)))
}

/// The `jwt()` middleware and the authorized-client check on `/api/*`.
pub(super) async fn guard(State(s): AppState, request: Request, next: Next) -> Response {
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::to_owned);
    if !authorized(&s.config(), token.as_deref()) {
        return unauthorized();
    }
    next.run(request).await
}

/// `POST /auth/{id}`: asks the user (AUTH_AT_FIRST), then issues a token.
pub(super) async fn authorize(
    State(s): AppState,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    Path(id): Path<String>,
) -> Response {
    let config = s.config();
    let mut clients: Vec<Value> = config["authorizedClients"].as_array().cloned().unwrap_or_default();
    let known = clients.iter().any(|c| c == id.as_str());
    if !known && config["authStrategy"] == AUTH_AT_FIRST && !ask_user(&s.app, &id, remote).await {
        return (StatusCode::FORBIDDEN, ()).into_response();
    }
    if !known {
        clients.push(json!(id));
        s.app.state::<Config>().set(&s.app, "plugins.api-server.authorizedClients", json!(clients));
    }
    let payload = json!({ "id": id, "iat": now_seconds() });
    let token = jwt::sign(&payload, config["secret"].as_str().unwrap_or_default());
    Json(json!({ "accessToken": token })).into_response()
}

/// The Allow/Deny message box; translations come from the page.
async fn ask_user(app: &AppHandle, id: &str, remote: SocketAddr) -> bool {
    let key = "plugins.api-server.dialog.request";
    let message = label(app, &format!("{key}.message"), "Allow {ID} ({origin}) to access the API?")
        .replace("{ID}", id)
        .replace("{origin}", &remote.ip().to_string());
    let (tx, rx) = oneshot::channel();
    app.dialog()
        .message(message)
        .title(label(app, &format!("{key}.title"), "API authorization request"))
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(
            label(app, &format!("{key}.buttons.allow"), "Allow"),
            label(app, &format!("{key}.buttons.deny"), "Deny"),
        ))
        .show(move |allowed| {
            let _ = tx.send(allowed);
        });
    rx.await.unwrap_or(false)
}

fn now_seconds() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}
/// HS256 tokens, as `hono/jwt` signs and verifies them.
pub(super) mod jwt {
    use super::*;

    fn mac(secret: &str) -> Hmac<Sha256> {
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC takes any key length")
    }

    pub fn sign(payload: &Value, secret: &str) -> String {
        let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"HS256","typ":"JWT"}"#);
        let body = URL_SAFE_NO_PAD.encode(payload.to_string());
        let mut mac = mac(secret);
        mac.update(format!("{header}.{body}").as_bytes());
        let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
        format!("{header}.{body}.{signature}")
    }

    /// The payload of a valid token: HS256 signature, and `nbf`/`exp`/`iat`
    /// checked like `hono/jwt`'s `verify`.
    pub fn verify(token: &str, secret: &str) -> Option<Value> {
        let (signed, signature) = token.rsplit_once('.')?;
        let (header, body) = signed.split_once('.')?;
        let header: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(header).ok()?).ok()?;
        if header["alg"] != "HS256" {
            return None;
        }
        let mut mac = mac(secret);
        mac.update(signed.as_bytes());
        mac.verify_slice(&URL_SAFE_NO_PAD.decode(signature).ok()?).ok()?;
        let payload: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(body).ok()?).ok()?;
        let now = now_seconds() as f64;
        let after = |key: &str| payload[key].as_f64().is_some_and(|t| t > now);
        let before = |key: &str| payload[key].as_f64().is_some_and(|t| t <= now);
        if after("nbf") || before("exp") || after("iat") {
            return None;
        }
        Some(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_round_trip_and_reject_tampering() {
        let token = jwt::sign(&json!({ "id": "client", "iat": now_seconds() }), "secret");
        assert_eq!(jwt::verify(&token, "secret").unwrap()["id"], "client");
        assert!(jwt::verify(&token, "other").is_none());
        let mut tampered = token.clone();
        tampered.insert(token.find('.').unwrap() + 2, 'x');
        assert!(jwt::verify(&tampered, "secret").is_none());
    }

    #[test]
    fn tokens_issued_in_the_future_are_rejected() {
        let token = jwt::sign(&json!({ "id": "c", "iat": now_seconds() + 3600 }), "s");
        assert!(jwt::verify(&token, "s").is_none());
    }

    #[test]
    fn hono_tokens_verify() {
        // hono/jwt `sign({ id: 'abc', iat: 1700000000 }, 'secret')`, so tokens
        // issued by the Electron build keep working
        let token = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpZCI6ImFiYyIsImlhdCI6MTcwMDAwMDAwMH0.zM9wV94fmJaKszKyImSMjfDzvJrVXm1pccDpyXzu8to";
        assert_eq!(jwt::verify(token, "secret").unwrap()["id"], "abc");
    }

    #[test]
    fn auth_checks_the_client_list() {
        let token = jwt::sign(&json!({ "id": "c1", "iat": 0 }), "s");
        let config = json!({ "authStrategy": AUTH_AT_FIRST, "secret": "s", "authorizedClients": ["c1"] });
        assert!(authorized(&config, Some(&token)));
        assert!(!authorized(&json!({ "authStrategy": AUTH_AT_FIRST, "secret": "s", "authorizedClients": [] }), Some(&token)));
        assert!(!authorized(&config, None));
        assert!(authorized(&json!({ "authStrategy": NONE }), None));
    }
}
