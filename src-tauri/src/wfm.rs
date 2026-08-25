// ==============================================================================
// warframe.market client
// ==============================================================================
//
// `Wfm` is the single seam to warframe.market. A caller names an endpoint and
// gets a result; the rate limits, the Bearer-vs-`JWT` auth scheme, the CSRF
// dance and the blueprint-slug retry quirk all live behind the interface rather
// than in the ~40 command handlers that used to reach for them by hand.
//
// What stays *outside* this module, at the Tauri boundary: acquiring a session
// (the login webview + injected token scrape needs an `AppHandle`), persisting
// it to the OS keyring, the price-prefetch queue and its event emission, and the
// multi-source pricing glue. Those speak to the OS or to the app; `Wfm` speaks
// only WFM.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tracing::{debug, info, warn};

const API_BASE: &str = "https://api.warframe.market";
const USER_AGENT: &str = "FrameForge/3.2.0";

// ==============================================================================
// Wire types
// ==============================================================================

/// The warframe.market login state — a token bundle, never the credentials that
/// produced it. Held in memory only; the boundary persists it to the keyring.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct WfmSession {
    pub access_token: String,
    pub refresh_token: String,
    pub client_id: String,
    pub device_id: String,
    pub username: String,
    pub status: String, // "online" | "ingame" | "invisible" | "offline"
    /// v1 JWT captured from the Authorization response header during signin.
    /// v1 endpoints (/v1/auctions/create etc.) require this; they reject v2 OAuth Bearer tokens.
    #[serde(default)]
    pub v1_jwt: String,
    /// CSRF token from the page <meta name="csrf-token"> captured after login.
    /// Required as x-csrftoken header on mutating WFM API calls (PUT, DELETE).
    #[serde(default)]
    pub csrf_token: String,
}

impl WfmSession {
    pub fn auth_header(&self) -> String {
        format!("Bearer {}", self.access_token)
    }
    /// Auth header for v1 WFM endpoints. WFM v1 uses "JWT <token>" scheme, not Bearer.
    pub fn v1_auth_header(&self) -> String {
        if !self.v1_jwt.is_empty() {
            format!("JWT {}", self.v1_jwt)
        } else {
            format!("Bearer {}", self.access_token)
        }
    }
}

#[derive(serde::Serialize)]
pub struct WfmItem {
    pub id: String,
    pub item_name: String,
    pub url_name: String,
}

#[derive(serde::Deserialize)]
pub struct WfmRivenAttribute {
    url_name: String,
    positive: bool,
    value: f64,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct WfmTopItem {
    pub name: String,
    pub url_name: String,
    pub image_name: Option<String>,
    pub unit_price: u32,  // median sell price (plat)
    pub daily_volume: f64, // average trades/day over last 7 days
    pub total_value_7d: u64, // unit_price × total volume over 7 days
}

#[derive(serde::Serialize)]
pub struct WfmPrice {
    pub url_name: String,
    pub sell_median: Option<f64>,
    pub buy_median: Option<f64>,
}

// ==============================================================================
// Rate limiter
// ==============================================================================
//
// A sliding-window limiter. `try_acquire` records a slot or reports how long to
// sleep; the caller sleeps *after* the lock is released so no request blocks
// another while merely waiting its turn.

struct RateLimiter {
    times: VecDeque<Instant>,
    limit: usize,
    window: Duration,
}

impl RateLimiter {
    fn new(limit: usize, window: Duration) -> Self {
        Self { times: VecDeque::new(), limit, window }
    }

    /// Returns None if a slot is available (and records the timestamp),
    /// or Some(duration) if the caller should sleep before retrying.
    fn try_acquire(&mut self) -> Option<Duration> {
        let now = Instant::now();
        while let Some(&front) = self.times.front() {
            if now.duration_since(front) >= self.window {
                self.times.pop_front();
            } else {
                break;
            }
        }
        if self.times.len() < self.limit {
            self.times.push_back(now);
            None
        } else {
            let oldest = *self.times.front().expect("times.len() >= limit >= 1, so the window is non-empty");
            Some(self.window.saturating_sub(now.duration_since(oldest)) + Duration::from_millis(10))
        }
    }
}

// ==============================================================================
// The client
// ==============================================================================

pub struct Wfm {
    session: Mutex<Option<WfmSession>>,
    /// General limit: ≤3 requests/second across every WFM endpoint.
    limiter: Mutex<RateLimiter>,
    /// Contract limit: ≤10 requests/minute for /v1/auctions/... (rivens, liches, sisters).
    auction_limiter: Mutex<RateLimiter>,
    /// slug → median sell price (None = not listed). Shared with the prefetch thread.
    price_cache: Mutex<std::collections::HashMap<String, Option<u32>>>,
    /// Top-items-by-volume result, cached in memory for the session.
    top_cache: Mutex<Option<(Instant, Vec<WfmTopItem>)>>,
    /// Prime-set (name, slug) pairs, fetched once per session.
    prime_sets_cache: Mutex<Option<Vec<(String, String)>>>,
    /// Short-lived response memo, keyed by endpoint + params. One popup open
    /// repeats its fetches (StrictMode double-mount, re-render refetches). The
    /// memo collapses them so they do not wait for the 3/sec limit.
    memo: Mutex<std::collections::HashMap<String, (Instant, serde_json::Value)>>,
    /// Per-key in-flight locks. Concurrent misses on the same memo key fetch
    /// once instead of racing. Entries are never removed: the map holds only
    /// a few keys for each item the user opens.
    memo_flights: Mutex<std::collections::HashMap<String, std::sync::Arc<Mutex<()>>>>,
}

impl Default for Wfm {
    fn default() -> Self {
        Self {
            session: Mutex::new(None),
            limiter: Mutex::new(RateLimiter::new(3, Duration::from_secs(1))),
            auction_limiter: Mutex::new(RateLimiter::new(10, Duration::from_secs(60))),
            price_cache: Mutex::new(std::collections::HashMap::new()),
            memo: Mutex::new(std::collections::HashMap::new()),
            memo_flights: Mutex::new(std::collections::HashMap::new()),
            top_cache: Mutex::new(None),
            prime_sets_cache: Mutex::new(None),
        }
    }
}

impl Wfm {
    pub fn new() -> Self {
        Self::default()
    }

    // ── Rate limiting (internal) ──────────────────────────────────────────────

    /// Block until the general 3/sec budget allows another request.
    fn wait(&self) {
        // The sleep here does not show in request spans: a span looks fast
        // while a burst waits for the limiter. Log the total blocked time
        // whenever it is non-zero.
        let start = Instant::now();
        let mut slept = false;
        loop {
            let sleep_dur = self.limiter.lock().unwrap_or_else(|e| e.into_inner()).try_acquire();
            match sleep_dur {
                None => break,
                Some(d) => {
                    slept = true;
                    std::thread::sleep(d);
                }
            }
        }
        if slept {
            tracing::debug!(blocked_ms = start.elapsed().as_millis() as u64, "rate limiter wait");
        }
    }

    /// Block until both the general 3/sec and the auction 10/min budgets allow one.
    fn auction_wait(&self) {
        self.wait();
        loop {
            let sleep_dur = self
                .auction_limiter
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .try_acquire();
            match sleep_dur {
                None => break,
                Some(d) => std::thread::sleep(d),
            }
        }
    }

    // ── Response memo (internal) ──────────────────────────────────────────────

    /// Return the memoized response for `key` if it is younger than 30 s.
    /// Otherwise run `fetch` (single-flight per key) and memoize the result.
    /// 30 s keeps order lists usable and is long enough to absorb a popup's
    /// burst.
    fn memoized(
        &self,
        key: &str,
        fetch: impl FnOnce() -> Result<serde_json::Value, String>,
    ) -> Result<serde_json::Value, String> {
        const MEMO_TTL: Duration = Duration::from_secs(30);
        let lookup = || {
            self.memo
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(key)
                .filter(|(at, _)| at.elapsed() < MEMO_TTL)
                .map(|(_, v)| v.clone())
        };
        if let Some(v) = lookup() {
            tracing::debug!(key, "memo hit");
            return Ok(v);
        }
        let flight = self
            .memo_flights
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(key.to_string())
            .or_default()
            .clone();
        let _guard = flight.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(v) = lookup() {
            tracing::debug!(key, "memo hit after flight");
            return Ok(v);
        }
        let v = fetch()?;
        self.memo
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key.to_string(), (Instant::now(), v.clone()));
        Ok(v)
    }

    // ── Request building (internal) ───────────────────────────────────────────

    fn request(&self, method: &str, path: &str, auth_header: &str) -> ureq::Request {
        let url = format!("{}{}", API_BASE, path);
        let req = match method {
            "POST" => ureq::post(&url),
            "PUT" => ureq::put(&url),
            "PATCH" => ureq::patch(&url),
            "DELETE" => ureq::delete(&url),
            _ => ureq::get(&url),
        };
        req.set("Authorization", auth_header)
            .set("Content-Type", "application/json")
            .set("Accept", "application/json")
            .set("language", "en")
            .set("platform", "pc")
            .set("User-Agent", USER_AGENT)
    }

    #[tracing::instrument(level = "debug", skip_all, fields(method = %method, path = %path))]
    fn call(&self, method: &str, path: &str, auth_header: &str) -> Result<ureq::Response, ureq::Error> {
        self.request(method, path, auth_header).call()
    }

    #[tracing::instrument(level = "debug", skip_all, fields(method = %method, path = %path))]
    fn send_json(
        &self,
        method: &str,
        path: &str,
        auth_header: &str,
        body: impl serde::Serialize,
    ) -> Result<ureq::Response, ureq::Error> {
        self.request(method, path, auth_header).send_json(body)
    }

    // ── Auth derivation (internal) ────────────────────────────────────────────

    /// Bearer auth header, or an error when there is no session.
    fn auth(&self) -> Result<String, String> {
        self.session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|s| s.auth_header())
            .ok_or_else(|| "Not logged in to warframe.market".into())
    }

    /// v1 ("JWT ...") auth header, or an error when there is no session.
    fn v1_auth(&self) -> Result<String, String> {
        self.session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|s| s.v1_auth_header())
            .ok_or_else(|| "Not logged in to warframe.market".into())
    }

    /// Bearer auth header when logged in, or `None` — for read endpoints that
    /// work anonymously but return richer data with a session.
    fn auth_opt(&self) -> Option<String> {
        self.session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|s| s.auth_header())
    }

    /// The raw access token, or an error when there is no session.
    fn access_token(&self) -> Result<String, String> {
        self.session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|s| s.access_token.clone())
            .ok_or_else(|| "Not logged in".into())
    }

    // ── Session lifecycle ─────────────────────────────────────────────────────

    fn set_session(&self, s: WfmSession) {
        *self.session.lock().unwrap_or_else(|e| e.into_inner()) = Some(s);
    }

    pub fn clear_session(&self) {
        *self.session.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// (username, status) for the current session, or None if not logged in.
    pub fn identity(&self) -> Option<(String, String)> {
        self.session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|s| (s.username.clone(), s.status.clone()))
    }

    /// Serialize the current token bundle to the JSON shape the boundary saves.
    pub fn token_json(&self) -> Option<String> {
        self.session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|s| {
                serde_json::json!({
                    "accessToken":  s.access_token,
                    "refreshToken": s.refresh_token,
                    "clientId":     s.client_id,
                    "deviceId":     s.device_id,
                    "v1Jwt":        s.v1_jwt,
                    "csrfToken":    s.csrf_token,
                })
                .to_string()
            })
    }

    /// Overwrite the in-memory status so `identity()` reflects a status change.
    fn set_cached_status(&self, status: String) {
        if let Some(s) = self.session.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            s.status = status;
        }
    }

    /// GET /v2/me with a Bearer token and return the parsed body. The three
    /// callers that validate a token *before* it lives in the session can't route
    /// through `self.call` (which reads the token from the session), so they share
    /// this instead. `err_ctx` prefixes the failure string each caller wants.
    fn me(&self, access_token: &str, err_ctx: &str) -> Result<serde_json::Value, String> {
        self.wait();
        ureq::get(&format!("{}/v2/me", API_BASE))
            .set("Authorization", &format!("Bearer {}", access_token))
            .set("language", "en")
            .set("platform", "pc")
            .set("User-Agent", USER_AGENT)
            .call()
            .map_err(|e| format!("{}: {}", err_ctx, e))?
            .into_json()
            .map_err(|e| format!("Parse: {}", e))
    }

    /// Validate a token bundle against /v2/me and store it. Fetches the CSRF
    /// token from the site when the bundle carries a v1 JWT but no CSRF token.
    /// Returns (username, status).
    pub fn adopt_tokens(
        &self,
        access_token: String,
        refresh_token: String,
        client_id: String,
        device_id: String,
        v1_jwt: String,
        csrf_token: Option<String>,
    ) -> Result<(String, String), String> {
        let json = self.me(&access_token, "Profile")?;
        let username = json["data"]["ingameName"].as_str().unwrap_or("Tenno").to_string();
        let status = json["data"]["status"].as_str().unwrap_or("offline").to_string();

        // The injected script captures the CSRF token from the meta tag as a
        // best-effort fallback; if that failed (SPA timing) fetch it directly.
        let csrf = csrf_token.unwrap_or_default();
        let csrf = if !csrf.is_empty() { csrf } else { self.fetch_csrf(&v1_jwt).unwrap_or_default() };
        info!(len = csrf.len(), "csrf_token captured");

        self.set_session(WfmSession {
            access_token,
            refresh_token,
            client_id,
            device_id,
            username: username.clone(),
            status: status.clone(),
            v1_jwt,
            csrf_token: csrf,
        });
        Ok((username, status))
    }

    /// Restore a session from a saved token JSON string, validating via /v2/me.
    /// Returns (username, status).
    pub fn restore_from_json(&self, jwt: &str) -> Result<(String, String), String> {
        // Backward compat: an old save was the bare access token, not a JSON bundle.
        let data: serde_json::Value =
            serde_json::from_str(jwt).unwrap_or_else(|_| serde_json::json!({ "accessToken": jwt }));
        let access_token = data["accessToken"].as_str().unwrap_or(jwt).to_string();
        let refresh_token = data["refreshToken"].as_str().unwrap_or("").to_string();
        let client_id = data["clientId"].as_str().unwrap_or("").to_string();
        let device_id = data["deviceId"].as_str().unwrap_or("").to_string();
        let v1_jwt = data["v1Jwt"].as_str().unwrap_or("").to_string();
        let mut csrf_token = data["csrfToken"].as_str().unwrap_or("").to_string();

        let json = self.me(&access_token, "401")?;
        let username = json["data"]["ingameName"].as_str().unwrap_or("Tenno").to_string();
        let status = json["data"]["status"].as_str().unwrap_or("offline").to_string();

        if csrf_token.is_empty() && !v1_jwt.is_empty() {
            debug!("restore: no saved token, fetching from site");
            csrf_token = self.fetch_csrf(&v1_jwt).unwrap_or_default();
            debug!(len = csrf_token.len(), "restore: csrf_token fetched");
        }
        self.set_session(WfmSession {
            access_token,
            refresh_token,
            client_id,
            device_id,
            username: username.clone(),
            status: status.clone(),
            v1_jwt,
            csrf_token,
        });
        Ok((username, status))
    }

    /// Log in via v1 signin (current recommended method per WFM Discord).
    /// The token arrives in the set-cookie header as "JWT=eyJ...".
    /// Returns the ingame username.
    pub fn login(&self, email: &str, password: &str) -> Result<String, String> {
        let body = serde_json::json!({ "email": email, "password": password });
        self.wait();
        let resp = ureq::post(&format!("{}/v1/auth/signin", API_BASE))
            .set("Content-Type", "application/json")
            .set("Authorization", "JWT")
            .set("User-Agent", USER_AGENT)
            .send_string(&body.to_string())
            .map_err(|e| format!("Login failed: {}", e))?;

        let token = resp
            .header("set-cookie")
            .and_then(|h| h.split(';').next())
            .and_then(|s| s.strip_prefix("JWT="))
            .map(|s| s.to_string())
            .ok_or("No JWT token in response cookies")?;

        let json: serde_json::Value = resp.into_json().map_err(|e| format!("Parse: {}", e))?;
        let username = json["payload"]["user"]["ingame_name"].as_str().unwrap_or("Tenno").to_string();
        let status = json["payload"]["user"]["status"].as_str().unwrap_or("offline").to_string();

        self.set_session(WfmSession {
            v1_jwt: token.clone(), // v1 login: JWT is the auth token for v1 endpoints
            csrf_token: String::new(),
            access_token: token,
            refresh_token: String::new(),
            client_id: String::new(),
            device_id: String::new(),
            username: username.clone(),
            status,
        });
        Ok(username)
    }

    /// Use the stored refresh token to silently mint a new access token.
    pub fn refresh(&self) -> Result<(), String> {
        let (refresh_token, client_id, device_id) = {
            let lock = self.session.lock().unwrap_or_else(|e| e.into_inner());
            let s = lock.as_ref().ok_or("Not logged in")?;
            (s.refresh_token.clone(), s.client_id.clone(), s.device_id.clone())
        };
        if refresh_token.is_empty() {
            return Err("No refresh token".into());
        }
        let body = serde_json::json!({
            "grantType": "refresh_token",
            "clientId": client_id,
            "deviceId": device_id,
            "refreshToken": refresh_token,
        });
        self.wait();
        let json: serde_json::Value = ureq::post(&format!("{}/auth/refresh", API_BASE))
            .set("Content-Type", "application/json")
            .set("User-Agent", USER_AGENT)
            .send_string(&body.to_string())
            .map_err(|e| format!("Refresh: {}", e))?
            .into_json()
            .map_err(|e| format!("Parse: {}", e))?;
        let new_access = json["data"]["accessToken"].as_str().ok_or("No accessToken")?.to_string();
        let new_refresh = json["data"]["refreshToken"].as_str().unwrap_or(&refresh_token).to_string();
        let mut lock = self.session.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = lock.as_mut() {
            s.access_token = new_access;
            s.refresh_token = new_refresh;
        }
        Ok(())
    }

    /// Fetch the CSRF token by loading the authenticated warframe.market page and
    /// scraping the `<meta name="csrf-token">` tag, falling back to the token
    /// embedded in the JWT payload when the page fetch or parse fails.
    #[tracing::instrument(level = "debug", skip_all)]
    fn fetch_csrf(&self, jwt: &str) -> Option<String> {
        if jwt.is_empty() {
            return None;
        }
        let resp = ureq::get("https://warframe.market/")
            .set("Cookie", &format!("JWT={}", jwt))
            .set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .set("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
            .call();
        let html = match resp {
            Ok(r) => {
                debug!("site fetch status=200");
                r.into_string().ok()?
            }
            Err(e) => {
                warn!(error = %e, "site fetch failed, trying JWT payload fallback");
                return jwt_payload_field(jwt, "csrf_token");
            }
        };
        if let Some(token) = parse_csrf_from_html(&html) {
            debug!(len = token.len(), "found meta token");
            return Some(token);
        }
        warn!(len = html.len(), "meta tag not found in HTML, trying JWT payload fallback");
        jwt_payload_field(jwt, "csrf_token")
    }

    /// Fetch the user's actual current status from WFM (/v2/me).
    /// Returns one of: "online" | "ingame" | "invisible" | "offline".
    pub fn fetch_status(&self) -> Result<String, String> {
        let token = self.access_token()?;
        let json = self.me(&token, "Status fetch")?;
        Ok(json["data"]["status"].as_str().unwrap_or("offline").to_string())
    }

    /// Set WFM online status via WebSocket. Connects, authenticates, sends the
    /// status with a 6-hour duration (so it persists after disconnect), then
    /// closes. Values: "online" | "ingame" | "invisible".
    pub fn set_status(&self, status: &str) -> Result<(), String> {
        if !["online", "ingame", "invisible"].contains(&status) {
            return Err("Status must be: online, ingame, or invisible".into());
        }
        let token = self.access_token()?;
        let status_for_ws = status.to_string();

        use tungstenite::{client::IntoClientRequest, stream::MaybeTlsStream, Message};
        use std::net::TcpStream;

        const HOST: &str = "ws.warframe.market:443";
        const RW_TIMEOUT: Duration = Duration::from_secs(5);

        let addr = HOST
            .parse::<std::net::SocketAddr>()
            .or_else(|_| {
                use std::net::ToSocketAddrs;
                HOST.to_socket_addrs()?
                    .next()
                    .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no addr"))
            })
            .map_err(|e| format!("DNS: {}", e))?;
        let tcp = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
            .map_err(|e| format!("TCP connect: {}", e))?;
        tcp.set_read_timeout(Some(RW_TIMEOUT)).ok();
        tcp.set_write_timeout(Some(RW_TIMEOUT)).ok();

        let req = "wss://ws.warframe.market/socket"
            .into_client_request()
            .map_err(|e| format!("WS request: {}", e))?;
        let (mut ws, _) = tungstenite::client_tls(req, tcp).map_err(|e| format!("WS connect: {}", e))?;

        match ws.get_ref() {
            MaybeTlsStream::Plain(s) => {
                let _ = s.set_read_timeout(Some(RW_TIMEOUT));
            }
            MaybeTlsStream::NativeTls(s) => {
                let _ = s.get_ref().set_read_timeout(Some(RW_TIMEOUT));
            }
            _ => {}
        }

        let send = |ws: &mut tungstenite::WebSocket<_>, route: &str, payload: serde_json::Value| {
            let msg = serde_json::json!({ "route": route, "payload": payload, "id": route }).to_string();
            ws.send(Message::Text(msg.into())).map_err(|e| format!("WS send: {}", e))
        };

        let wait_for = |ws: &mut tungstenite::WebSocket<_>, ok_route: &str, err_route: &str| -> Result<(), String> {
            for _ in 0..20 {
                match ws.read() {
                    Ok(Message::Text(text)) => {
                        let v: serde_json::Value = serde_json::from_str(text.as_str()).unwrap_or_default();
                        let route = v["route"].as_str().unwrap_or("");
                        if route == ok_route {
                            return Ok(());
                        }
                        if route == err_route {
                            return Err(format!("WFM error: {}", v["payload"]));
                        }
                    }
                    Err(e) => return Err(format!("WS read: {}", e)),
                    _ => {}
                }
            }
            Err("WS response timeout".into())
        };

        send(&mut ws, "@wfm|cmd/auth/signIn", serde_json::json!({ "token": token }))?;
        wait_for(&mut ws, "@wfm|cmd/auth/signIn:ok", "@wfm|cmd/auth/signIn:error")?;

        send(
            &mut ws,
            "@wfm|cmd/status/set",
            serde_json::json!({ "status": status_for_ws, "duration": 21600 }),
        )?;
        wait_for(&mut ws, "@wfm|cmd/status/set:ok", "@wfm|cmd/status/set:error")?;

        let _ = ws.close(None);
        self.set_cached_status(status.to_string());
        Ok(())
    }

    // ── Orders ────────────────────────────────────────────────────────────────

    /// Current buy + sell orders for an item, each sorted best-first and capped
    /// at 15. When `mod_rank` is set, results are filtered to that rank.
    #[tracing::instrument(level = "debug", skip_all, fields(slug = %url_name))]
    pub fn item_orders(&self, url_name: &str, mod_rank: Option<u32>) -> Result<serde_json::Value, String> {
        // Memoize the raw order list per item, not per rank. A mod-rank change
        // in the popup then filters the memoized data and does not fetch again.
        let json = self.memoized(&format!("orders:{url_name}"), || {
            let auth = self.auth_opt();
            self.wait();
            let mut req = ureq::get(&format!("{}/v2/orders/item/{}", API_BASE, url_name))
                .set("language", "en")
                .set("platform", "pc")
                .set("User-Agent", USER_AGENT);
            if let Some(ref h) = auth {
                req = req.set("Authorization", h);
            }
            req.call()
                .map_err(|e| format!("orders: {}", e))?
                .into_json()
                .map_err(|e| format!("parse: {}", e))
        })?;

        // Present orders best-first: ingame sellers before online before offline,
        // then cheapest sell / richest buy.
        fn status_rank(o: &serde_json::Value) -> u8 {
            match o["user"]["status"].as_str().unwrap_or("offline") {
                "ingame" => 0,
                "online" => 1,
                _ => 2,
            }
        }
        let all_orders = json["data"].as_array().cloned().unwrap_or_default();
        let orders: Vec<serde_json::Value> = if let Some(rank) = mod_rank {
            all_orders
                .into_iter()
                .filter(|o| o["rank"].as_u64().map(|r| r as u32 == rank).unwrap_or(false))
                .collect()
        } else {
            all_orders
        };
        let mut sell: Vec<serde_json::Value> = orders.iter().filter(|o| o["type"] == "sell").cloned().collect();
        sell.sort_by(|a, b| {
            status_rank(a).cmp(&status_rank(b)).then_with(|| {
                a["platinum"].as_i64().unwrap_or(999_999).cmp(&b["platinum"].as_i64().unwrap_or(999_999))
            })
        });
        let mut buy: Vec<serde_json::Value> = orders.iter().filter(|o| o["type"] == "buy").cloned().collect();
        buy.sort_by(|a, b| {
            status_rank(a).cmp(&status_rank(b)).then_with(|| {
                b["platinum"].as_i64().unwrap_or(0).cmp(&a["platinum"].as_i64().unwrap_or(0))
            })
        });
        Ok(serde_json::json!({
            "sell": sell.into_iter().take(15).collect::<Vec<_>>(),
            "buy": buy.into_iter().take(15).collect::<Vec<_>>(),
        }))
    }

    /// 90-day daily price statistics for an item (for the chart).
    #[tracing::instrument(level = "debug", skip_all, fields(slug = %url_name))]
    pub fn item_statistics(&self, url_name: &str) -> Result<serde_json::Value, String> {
        self.memoized(&format!("stats:{url_name}"), || {
            let auth = self.auth_opt();
            self.wait();
            let mut req = ureq::get(&format!("{}/v1/items/{}/statistics", API_BASE, url_name))
                .set("language", "en")
                .set("platform", "pc")
                .set("User-Agent", USER_AGENT);
            if let Some(ref h) = auth {
                req = req.set("Authorization", h);
            }
            let json: serde_json::Value = req
                .call()
                .map_err(|e| format!("stats: {}", e))?
                .into_json()
                .map_err(|e| format!("parse: {}", e))?;
            Ok(json["payload"]["statistics_closed"]["90days"].clone())
        })
    }

    /// The internal WFM item detail for a slug (needed to create orders). The
    /// caller enriches it further; `Wfm` returns only what the wire gives.
    #[tracing::instrument(level = "debug", skip_all, fields(slug = %url_name))]
    pub fn item_info(&self, url_name: &str) -> Result<serde_json::Value, String> {
        self.memoized(&format!("info:{url_name}"), || {
            let auth = self.auth_opt().unwrap_or_default();
            self.wait();
            self.call("GET", &format!("/v2/items/{}", url_name), &auth)
                .map_err(|e| format!("Item info: {}", e))?
                .into_json::<serde_json::Value>()
                .map_err(|e| format!("Parse: {}", e))
                .map(|j| j["data"].clone())
        })
    }

    /// The authenticated user's active buy + sell orders.
    pub fn my_orders(&self) -> Result<serde_json::Value, String> {
        let auth = self.auth()?;
        self.wait();
        let json: serde_json::Value = self
            .call("GET", "/v2/orders/my", &auth)
            .map_err(|e| format!("Get orders: {}", e))?
            .into_json()
            .map_err(|e| format!("Parse: {}", e))?;
        Ok(json["data"].clone())
    }

    /// Raw authenticated GET, pretty-printed — for the debug console only.
    pub fn debug_dump(&self, path: &str) -> Result<String, String> {
        let auth = self.auth()?;
        self.wait();
        let json: serde_json::Value = self
            .call("GET", path, &auth)
            .map_err(|e| format!("Dump: {}", e))?
            .into_json()
            .map_err(|e| format!("Parse: {}", e))?;
        serde_json::to_string_pretty(&json).map_err(|e| e.to_string())
    }

    /// Create a buy or sell order. `mod_rank` must be set for mods — WFM 400s without it.
    pub fn create_order(
        &self,
        item_id: &str,
        order_type: &str,
        platinum: u32,
        quantity: u32,
        visible: bool,
        mod_rank: Option<u32>,
    ) -> Result<serde_json::Value, String> {
        let auth = self.auth()?;
        let mut body = serde_json::json!({
            "itemId": item_id, "type": order_type, "platinum": platinum,
            "quantity": quantity, "visible": visible,
        });
        if let Some(rank) = mod_rank {
            body["rank"] = serde_json::json!(rank);
        }
        self.wait();
        self.send_json("POST", "/v2/order", &auth, &body)
            .map_err(|e| format!("Create order: {}", e))?
            .into_json::<serde_json::Value>()
            .map_err(|e| format!("Parse: {}", e))
            .map(|j| j["data"].clone())
    }

    /// Update an order's price, quantity, or visibility.
    pub fn update_order(
        &self,
        order_id: &str,
        platinum: u32,
        quantity: u32,
        visible: bool,
    ) -> Result<serde_json::Value, String> {
        let auth = self.auth()?;
        let body = serde_json::json!({ "platinum": platinum, "quantity": quantity, "visible": visible });
        self.wait();
        self.send_json("PATCH", &format!("/v2/order/{}", order_id), &auth, &body)
            .map_err(|e| format!("Update order: {}", e))?
            .into_json::<serde_json::Value>()
            .map_err(|e| format!("Parse: {}", e))
            .map(|j| j["data"].clone())
    }

    /// Delete an order.
    pub fn delete_order(&self, order_id: &str) -> Result<(), String> {
        let auth = self.auth()?;
        self.wait();
        self.call("DELETE", &format!("/v2/order/{}", order_id), &auth)
            .map_err(|e| format!("Delete order: {}", e))?;
        Ok(())
    }

    // ── Riven auctions (v1) ───────────────────────────────────────────────────

    /// Known riven attribute url_names, scraped from live auction listings
    /// (/v1/riven/attributes was removed by WFM).
    pub fn riven_attributes(&self) -> Result<Vec<String>, String> {
        self.auction_wait();
        let json: serde_json::Value = ureq::get(&format!("{}/v1/auctions/search", API_BASE))
            .query("type", "riven")
            .set("language", "en")
            .set("platform", "pc")
            .set("User-Agent", USER_AGENT)
            .call()
            .map_err(|e| format!("Search: {}", e))?
            .into_json()
            .map_err(|e| format!("Parse: {}", e))?;
        let mut seen = std::collections::HashSet::new();
        if let Some(auctions) = json["payload"]["auctions"].as_array() {
            for auction in auctions {
                if let Some(attrs) = auction["item"]["attributes"].as_array() {
                    for attr in attrs {
                        if let Some(url) = attr["url_name"].as_str() {
                            seen.insert(url.to_string());
                        }
                    }
                }
            }
        }
        let mut list: Vec<String> = seen.into_iter().collect();
        list.sort();
        Ok(list)
    }

    /// Post a revealed riven as an auction. Returns the full response JSON; the
    /// caller extracts the new auction id for its own bookkeeping.
    #[allow(clippy::too_many_arguments)]
    pub fn create_riven_auction(
        &self,
        weapon_url_name: &str,
        riven_name: &str,
        mastery_level: u32,
        mod_rank: u8,
        re_rolls: u32,
        polarity: &str,
        attributes: &[WfmRivenAttribute],
        starting_price: u32,
        buyout_price: Option<u32>,
        minimal_reputation: u32,
        note: &str,
        visible: bool,
        is_direct_sell: bool,
    ) -> Result<serde_json::Value, String> {
        let auth = self.v1_auth()?;
        let attrs: Vec<serde_json::Value> = attributes
            .iter()
            .map(|a| serde_json::json!({ "url_name": a.url_name, "positive": a.positive, "value": a.value }))
            .collect();
        let mut payload = serde_json::json!({
            "item": {
                "type": "riven",
                "weapon_url_name": weapon_url_name,
                "name": riven_name,
                "mastery_level": mastery_level,
                "mod_rank": mod_rank,
                "re_rolls": re_rolls,
                "polarity": polarity,
                "attributes": attrs,
            },
            "starting_price": starting_price,
            "minimal_reputation": minimal_reputation,
            "note": note,
            "visible": visible,
            "is_direct_sell": is_direct_sell,
        });
        // WFM v1 requires buyout_price to be present (null = no buyout).
        payload["buyout_price"] = serde_json::json!(buyout_price);
        self.auction_wait();
        let resp = self
            .send_json("POST", "/v1/auctions/create", &auth, payload)
            .map_err(auction_error("Create riven auction"))?;
        resp.into_json().map_err(|e| format!("Parse auction response: {}", e))
    }

    /// Switch an auction between Auction and Direct Sale by close-and-recreate
    /// (WFM's update endpoint won't change is_direct_sell). Fetches the full
    /// entry first so attributes and polarity carry over. Returns the new
    /// auction JSON; the caller already holds the now-closed id it passed in.
    pub fn switch_riven_type(
        &self,
        auction_id: &str,
        new_is_direct_sell: bool,
        starting_price: u32,
        buyout_price: Option<u32>,
        visible: bool,
    ) -> Result<serde_json::Value, String> {
        let auth = self.v1_auth()?;

        // Fetch the full detail so every field (attributes, polarity, ...) carries over.
        self.auction_wait();
        let entry: serde_json::Value = self
            .call("GET", &format!("/v1/auctions/entry/{}", auction_id), &auth)
            .map_err(auction_error("Fetch auction"))?
            .into_json()
            .map_err(|e| format!("Parse auction entry: {}", e))?;

        let auction = &entry["payload"]["auction"];
        let item = &auction["item"];
        let item_payload = serde_json::json!({
            "type":            "riven",
            "weapon_url_name": item["weapon_url_name"],
            "name":            item["name"],
            "mastery_level":   item["mastery_level"],
            "mod_rank":        item["mod_rank"],
            "re_rolls":        item["re_rolls"],
            "polarity":        item["polarity"],
            "attributes":      item["attributes"],
        });
        let note = auction["note"].as_str().unwrap_or("").to_string();
        let minimal_reputation = auction["minimal_reputation"].as_u64().unwrap_or(0) as u32;

        // Close the old auction.
        self.auction_wait();
        self.call("PUT", &format!("/v1/auctions/entry/{}/close", auction_id), &auth)
            .map_err(auction_error("Delete auction"))?;

        // Create the replacement with the chosen type.
        let mut payload = serde_json::json!({
            "item":               item_payload,
            "starting_price":     starting_price,
            "minimal_reputation": minimal_reputation,
            "note":               note,
            "visible":            visible,
            "is_direct_sell":     new_is_direct_sell,
        });
        payload["buyout_price"] = serde_json::json!(buyout_price);

        self.auction_wait();
        let resp = self
            .send_json("POST", "/v1/auctions/create", &auth, payload)
            .map_err(auction_error("Create riven auction"))?;
        resp.into_json().map_err(|e| format!("Parse auction response: {}", e))
    }

    /// Close (delete) a riven auction.
    pub fn delete_auction(&self, auction_id: &str) -> Result<(), String> {
        let auth = self.v1_auth()?;
        self.auction_wait();
        self.call("PUT", &format!("/v1/auctions/entry/{}/close", auction_id), &auth)
            .map_err(auction_error("Delete auction"))?;
        Ok(())
    }

    /// Update an auction's starting price, buyout price, and visibility.
    /// `buyout_price = None` clears the buyout.
    pub fn update_auction(
        &self,
        auction_id: &str,
        starting_price: u32,
        buyout_price: Option<u32>,
        visible: bool,
    ) -> Result<(), String> {
        let auth = self.v1_auth()?;
        let mut body = serde_json::json!({ "starting_price": starting_price, "visible": visible });
        body["buyout_price"] = buyout_price.map_or(serde_json::Value::Null, |v| serde_json::json!(v));
        self.auction_wait();
        self.send_json("PUT", &format!("/v1/auctions/entry/{}", auction_id), &auth, body)
            .map_err(auction_error("Update auction"))?;
        Ok(())
    }

    /// Toggle an auction's visibility.
    pub fn set_auction_visible(&self, auction_id: &str, visible: bool) -> Result<(), String> {
        let auth = self.v1_auth()?;
        self.auction_wait();
        self.send_json(
            "PUT",
            &format!("/v1/auctions/entry/{}", auction_id),
            &auth,
            serde_json::json!({ "visible": visible }),
        )
        .map_err(auction_error("Set auction visibility"))?;
        Ok(())
    }

    /// The current user's riven auctions. Phase 1 hits the profile endpoint
    /// (visible auctions); phase 2 fetches each stored id not already returned
    /// (hidden auctions FrameForge created), skipping closed ones.
    pub fn my_riven_auctions(&self, stored_ids: &[String]) -> Result<serde_json::Value, String> {
        let (v1_auth, username) = {
            let lock = self.session.lock().unwrap_or_else(|e| e.into_inner());
            let s = lock.as_ref().ok_or("Not logged in to warframe.market")?;
            (s.v1_auth_header(), s.username.clone())
        };

        self.auction_wait();
        let profile_resp: serde_json::Value = self
            .call("GET", &format!("/v1/profile/{}/auctions", username), &v1_auth)
            .map_err(|e| format!("Fetch auctions: {}", e))?
            .into_json()
            .map_err(|e| format!("Parse auctions: {}", e))?;

        let mut auctions: Vec<serde_json::Value> =
            profile_resp["payload"]["auctions"].as_array().cloned().unwrap_or_default();
        let seen_ids: std::collections::HashSet<String> =
            auctions.iter().filter_map(|a| a["id"].as_str().map(|s| s.to_string())).collect();

        for id in stored_ids {
            if seen_ids.contains(id) {
                continue;
            }
            self.auction_wait();
            let entry: serde_json::Value = match self.call("GET", &format!("/v1/auctions/entry/{}", id), &v1_auth) {
                Ok(r) => match r.into_json() {
                    Ok(j) => j,
                    Err(_) => continue,
                },
                Err(_) => continue,
            };
            if let Some(auction) = entry["payload"]["auction"].as_object() {
                if auction.get("closed").and_then(|v| v.as_bool()).unwrap_or(false) {
                    continue;
                }
                auctions.push(serde_json::Value::Object(auction.clone()));
            }
        }
        Ok(serde_json::json!({ "payload": { "auctions": auctions } }))
    }

    // ── Item list ─────────────────────────────────────────────────────────────

    /// The full warframe.market item list (v2; v1 /items 404s).
    pub fn items(&self) -> Result<Vec<WfmItem>, String> {
        self.wait();
        let json: serde_json::Value = ureq::get(&format!("{}/v2/items", API_BASE))
            .call()
            .map_err(|e| format!("wfm items: {}", e))?
            .into_json()
            .map_err(|e| format!("wfm items parse: {}", e))?;
        let items = json["data"]
            .as_array()
            .ok_or("no data array in v2 response")?
            .iter()
            .filter_map(|v| {
                Some(WfmItem {
                    id: v["id"].as_str().unwrap_or("").to_string(),
                    item_name: v["i18n"]["en"]["name"].as_str()?.to_string(),
                    url_name: v["slug"].as_str()?.to_string(),
                })
            })
            .collect();
        Ok(items)
    }

    // ── Pricing ───────────────────────────────────────────────────────────────

    /// 48-hour median sell price for a slug, or None if unlisted. Uses the 48h
    /// VWAP when there are ≥3 recent trades, else falls back to the 90-day window.
    #[tracing::instrument(level = "debug", skip_all, fields(slug = %slug))]
    pub fn price_for_slug(&self, slug: &str) -> Result<Option<u32>, String> {
        self.wait();
        let url = format!("{}/v1/items/{}/statistics", API_BASE, slug);
        match ureq::get(&url).call() {
            Ok(resp) => {
                let json: serde_json::Value = resp.into_json().map_err(|e| format!("wfm price parse: {}", e))?;
                let h48 = json["payload"]["statistics_closed"]["48hours"].as_array();
                let d90 = json["payload"]["statistics_closed"]["90days"].as_array();
                let vol_48: f64 =
                    h48.map(|arr| arr.iter().filter_map(|e| e["volume"].as_f64()).sum()).unwrap_or(0.0);
                let p = if vol_48 >= 3.0 {
                    h48.and_then(|arr| trimmed_median_from_stats(arr))
                } else {
                    None
                };
                Ok(p.or_else(|| d90.and_then(|arr| trimmed_median_from_stats(arr))))
            }
            Err(_) => Ok(None),
        }
    }

    /// Price for a slug, retrying the blueprint-suffix variant — WFM is
    /// inconsistent about whether component blueprints keep the "_blueprint" suffix.
    pub fn price_with_fallback(&self, slug: &str) -> Option<u32> {
        self.price_for_slug(slug).unwrap_or(None).or_else(|| {
            if let Some(stripped) = slug.strip_suffix("_blueprint") {
                self.price_for_slug(stripped).unwrap_or(None)
            } else {
                self.price_for_slug(&format!("{}_blueprint", slug)).unwrap_or(None)
            }
        })
    }

    /// 7-day price + average daily volume for a slug, or None if unlisted / stale.
    pub fn stats_7day(&self, slug: &str) -> Option<(u32, f64)> {
        self.wait();
        let url = format!("{}/v1/items/{}/statistics", API_BASE, slug);
        let json: serde_json::Value = ureq::get(&url)
            .timeout(Duration::from_secs(10))
            .call()
            .ok()?
            .into_json()
            .ok()?;
        let days = json["payload"]["statistics_closed"]["90days"].as_array()?;
        if days.is_empty() {
            return None;
        }
        let price = days.last()?.get("median")?.as_f64().map(|f| f.round() as u32)?;
        let vol_7d: f64 = days.iter().rev().take(7).filter_map(|e| e["volume"].as_f64()).sum();
        if vol_7d == 0.0 {
            return None;
        }
        Some((price, vol_7d / 7.0))
    }

    /// Prime-set (name, slug) pairs, fetched once per session and cached.
    pub fn prime_sets(&self) -> Vec<(String, String)> {
        {
            let guard = self.prime_sets_cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(ref sets) = *guard {
                return sets.clone();
            }
        }
        let sets = self.fetch_prime_sets();
        if !sets.is_empty() {
            *self.prime_sets_cache.lock().unwrap_or_else(|e| e.into_inner()) = Some(sets.clone());
        }
        sets
    }

    fn fetch_prime_sets(&self) -> Vec<(String, String)> {
        self.wait();
        let resp = ureq::get(&format!("{}/v2/items", API_BASE))
            .set("User-Agent", USER_AGENT)
            .timeout(Duration::from_secs(15))
            .call();
        let json: serde_json::Value = match resp {
            Ok(r) => match r.into_json() {
                Ok(v) => v,
                Err(_) => return Vec::new(),
            },
            Err(_) => return Vec::new(),
        };
        let items = match json["data"].as_array() {
            Some(a) => a,
            None => return Vec::new(),
        };
        items
            .iter()
            .filter_map(|item| {
                let name = item["i18n"]["en"]["name"].as_str()?;
                let url = item["slug"].as_str()?;
                let lower = name.to_lowercase();
                if lower.contains("prime") && lower.ends_with(" set") {
                    Some((name.to_string(), url.to_string()))
                } else {
                    None
                }
            })
            .collect()
    }

    // ── Price cache ───────────────────────────────────────────────────────────

    /// The cached price for a slug: `Some(price_opt)` when the slug was fetched
    /// (`price_opt` distinguishes "listed at N" from "not listed"), `None` when
    /// it has never been fetched.
    pub fn cached_price(&self, slug: &str) -> Option<Option<u32>> {
        self.price_cache.lock().unwrap_or_else(|e| e.into_inner()).get(slug).copied()
    }

    pub fn is_price_cached(&self, slug: &str) -> bool {
        self.price_cache.lock().unwrap_or_else(|e| e.into_inner()).contains_key(slug)
    }

    pub fn cache_price(&self, slug: String, price: Option<u32>) {
        self.price_cache.lock().unwrap_or_else(|e| e.into_inner()).insert(slug, price);
    }

    pub fn uncache_price(&self, slug: &str) {
        self.price_cache.lock().unwrap_or_else(|e| e.into_inner()).remove(slug);
    }

    /// A clone of the whole slug → price cache.
    pub fn cached_prices(&self) -> std::collections::HashMap<String, Option<u32>> {
        self.price_cache.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    // ── Top items cache ───────────────────────────────────────────────────────

    /// The in-memory top-items result if still within `max_age`.
    pub fn cached_top_items(&self, max_age: Duration) -> Option<Vec<WfmTopItem>> {
        let guard = self.top_cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((ts, ref items)) = *guard {
            if ts.elapsed() < max_age {
                return Some(items.clone());
            }
        }
        None
    }

    pub fn set_top_items(&self, items: Vec<WfmTopItem>) {
        *self.top_cache.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), items));
    }
}

// ==============================================================================
// Pure helpers (no session, no network) — the tested core
// ==============================================================================

/// Convert a display name to a warframe.market URL slug.
/// E.g. "Ash Prime Neuroptics Blueprint" → "ash_prime_neuroptics_blueprint"
pub fn to_wfm_slug(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c == ' ' { '_' } else { c })
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect()
}

/// Extract the `<meta name="csrf-token" content="...">` value from page HTML.
fn parse_csrf_from_html(html: &str) -> Option<String> {
    let needle = r#"name="csrf-token" content=""#;
    let start = html.find(needle)? + needle.len();
    let end_rel = html[start..].find('"')?;
    let token = &html[start..start + end_rel];
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

/// Decode a JWT payload (base64url, the middle segment) and read a string field.
fn jwt_payload_field(jwt: &str, field: &str) -> Option<String> {
    let parts: Vec<&str> = jwt.splitn(3, '.').collect();
    if parts.len() < 2 {
        return None;
    }
    let payload_b64 = parts[1];
    let padded = match payload_b64.len() % 4 {
        2 => format!("{}==", payload_b64),
        3 => format!("{}=", payload_b64),
        _ => payload_b64.to_string(),
    };
    let decoded = base64_decode_url(&padded)?;
    let json: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    json[field].as_str().map(|s| s.to_string())
}

/// Minimal base64url decoder (no external crate). Tolerates missing padding.
fn base64_decode_url(s: &str) -> Option<Vec<u8>> {
    let s = s.replace('-', "+").replace('_', "/");
    let chars: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=";
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 3 < bytes.len() {
        let c0 = chars.iter().position(|&c| c == bytes[i])? as u32;
        let c1 = chars.iter().position(|&c| c == bytes[i + 1])? as u32;
        let c2 = chars.iter().position(|&c| c == bytes[i + 2]).unwrap_or(64) as u32;
        let c3 = chars.iter().position(|&c| c == bytes[i + 3]).unwrap_or(64) as u32;
        out.push(((c0 << 2) | (c1 >> 4)) as u8);
        if c2 != 64 {
            out.push(((c1 << 4) | (c2 >> 2)) as u8);
        }
        if c3 != 64 {
            out.push(((c2 << 6) | c3) as u8);
        }
        i += 4;
    }
    Some(out)
}

/// Trimmed median of per-bucket price medians. Drops the cheapest and dearest
/// 15 % of buckets before taking the median, so a single outlier day (e.g. a mod
/// listed at 45 834 p) can't skew the price. Falls back to the full-set median
/// when there are too few points to trim.
fn trimmed_median_from_stats(arr: &[serde_json::Value]) -> Option<u32> {
    let mut prices: Vec<f64> = arr
        .iter()
        .filter_map(|e| e["median"].as_f64())
        .filter(|&p| p > 0.0)
        .collect();
    if prices.is_empty() {
        return None;
    }
    prices.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = prices.len();
    let cut = (n as f64 * 0.15).floor() as usize;
    let lo = cut;
    let hi = n.saturating_sub(cut);
    let slice = if lo < hi { &prices[lo..hi] } else { &prices[..] };
    let mid = slice.len() / 2;
    let median = if slice.len() % 2 == 0 {
        (slice[mid - 1] + slice[mid]) / 2.0
    } else {
        slice[mid]
    };
    Some(median.round() as u32)
}

/// Map a ureq auction error to the "<action>: HTTP <code>: <body>" message the
/// v1 auction commands all report, reading the response body for the reason.
fn auction_error(action: &'static str) -> impl Fn(ureq::Error) -> String {
    move |e| match e {
        ureq::Error::Status(code, r) => {
            let body = r.into_string().unwrap_or_default();
            format!("{}: HTTP {}: {}", action, code, body)
        }
        other => format!("{}: {}", action, other),
    }
}

// ==============================================================================
// Tests — the pure core, no network
// ==============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_lowercases_spaces_and_strips_punctuation() {
        assert_eq!(to_wfm_slug("Ash Prime Neuroptics Blueprint"), "ash_prime_neuroptics_blueprint");
        // Apostrophes and other punctuation are dropped, not replaced.
        assert_eq!(to_wfm_slug("Vay Hek's Frequency"), "vay_heks_frequency");
        assert_eq!(to_wfm_slug("Já Prime"), "j_prime"); // non-ascii dropped
    }

    #[test]
    fn csrf_parsed_from_meta_tag() {
        let html = r#"<head><meta name="csrf-token" content="abc123token"></head>"#;
        assert_eq!(parse_csrf_from_html(html).as_deref(), Some("abc123token"));
    }

    #[test]
    fn csrf_absent_or_empty_is_none() {
        assert_eq!(parse_csrf_from_html("<head></head>"), None);
        assert_eq!(parse_csrf_from_html(r#"<meta name="csrf-token" content="">"#), None);
    }

    #[test]
    fn jwt_payload_field_reads_a_claim() {
        // header.payload.signature — payload is {"csrf_token":"tok","x":1}, base64url no padding.
        let payload = "eyJjc3JmX3Rva2VuIjoidG9rIiwieCI6MX0";
        let jwt = format!("h.{}.s", payload);
        assert_eq!(jwt_payload_field(&jwt, "csrf_token").as_deref(), Some("tok"));
        assert_eq!(jwt_payload_field(&jwt, "missing"), None);
        assert_eq!(jwt_payload_field("not-a-jwt", "csrf_token"), None);
    }

    #[test]
    fn trimmed_median_ignores_price_outliers() {
        let bucket = |m: f64| serde_json::json!({ "median": m });
        // Nine ~10p days plus one 900p outlier — the outlier is trimmed away.
        let mut arr: Vec<serde_json::Value> = (0..9).map(|_| bucket(10.0)).collect();
        arr.push(bucket(900.0));
        assert_eq!(trimmed_median_from_stats(&arr), Some(10));
        // Too few points to trim → plain median.
        assert_eq!(trimmed_median_from_stats(&[bucket(5.0), bucket(7.0)]), Some(6));
        assert_eq!(trimmed_median_from_stats(&[]), None);
    }

    #[test]
    fn rate_limiter_admits_up_to_limit_then_defers() {
        let mut rl = RateLimiter::new(3, Duration::from_secs(1));
        assert!(rl.try_acquire().is_none());
        assert!(rl.try_acquire().is_none());
        assert!(rl.try_acquire().is_none());
        // Fourth within the window must be told to wait.
        assert!(rl.try_acquire().is_some());
    }
}
