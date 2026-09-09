/// Console / cross-platform login via warframe.com WebView.
///
/// How it works:
///   1. We open a Tauri window loading https://www.warframe.com/login.
///   2. An initialization script is injected before any page JS runs.
///      It wraps window.fetch and XMLHttpRequest so that when warframe.com
///      calls its own login.php (or OAuth callback pages), we intercept the
///      decrypted response — the server sends back plain JSON even though the
///      request body is encrypted with ezip.
///   3. When we see {"id": "...", "Nonce": ...} in a response, the script
///      fires a request to the `ffauth://` custom URI scheme (via a hidden
///      image load — avoids CORS restrictions from the https page).
///   4. Tauri's scheme handler extracts the credentials and emits
///      "console-login-success" to the main window, then closes the WebView.
///
/// To remove this feature entirely: delete this file, ConsoleLogin.tsx,
/// capabilities/console-login.json, and the 6 tagged lines in lib.rs / App.tsx.
use tauri::{Emitter, Manager};

/// Injected into the console-login WebView before any page scripts run.
pub const INIT_SCRIPT: &str = r#"(function () {
    'use strict';
    var _sent = false;
    var _origFetch = window.fetch;

    function trySend(id, nonce) {
        if (_sent || !id || !nonce || String(nonce) === 'null' || String(nonce) === '0') return;
        _sent = true;
        try {
            var img = new Image();
            img.src = 'ffauth://c?i=' + encodeURIComponent(String(id))
                    + '&n=' + encodeURIComponent(String(nonce));
        } catch (e) {}
    }

    function tryExtract(text) {
        try {
            var d = JSON.parse(text);
            if (!d) return;
            // Shape 1: { "id": "<24hexchars>", "Nonce": ..., ... }  (login.php response)
            var id = d.id || d.user_id || d.accountId || '';
            if (typeof id === 'string' && id.length === 24 && /^[0-9a-f]+$/.test(id)) {
                var nonce = (d.Nonce !== undefined) ? d.Nonce
                          : (d.Token  !== undefined) ? d.Token
                          : null;
                if (nonce !== null) { trySend(id, nonce); return; }
            }
        } catch (e) {}
    }

    // Wrap fetch to catch any API response that looks like login credentials.
    window.fetch = function (input, init) {
        return _origFetch.apply(this, arguments).then(function (resp) {
            resp.clone().text()
                .then(function (t) { tryExtract(t); })
                .catch(function () {});
            return resp;
        });
    };

    // Wrap XHR too.
    var _origOpen = XMLHttpRequest.prototype.open;
    XMLHttpRequest.prototype.open = function (method, url) {
        this._ffUrl = String(url || '');
        return _origOpen.apply(this, arguments);
    };
    var _origSend = XMLHttpRequest.prototype.send;
    XMLHttpRequest.prototype.send = function () {
        var self = this;
        self.addEventListener('load', function () { tryExtract(self.responseText); });
        return _origSend.apply(this, arguments);
    };

    // warframe.com login uses a native HTML <form> POST (full-page redirect),
    // so our fetch/XHR wrappers never see the login call itself.
    // After the redirect lands us on the homepage we run again fresh.
    // Detect this: if we are NOT on the /login page, the user just authenticated.
    // Probe known session endpoints — gid cookie is sent automatically (same-origin).
    var onLoginPage = /\/login($|[?#/])/.test(window.location.pathname);
    if (!onLoginPage) {
        var probeUrls = [
            'https://www.warframe.com/api/user-data',
            '/api/user-data',
            'https://api.warframe.com/api/user-data',
        ];
        // Small delay so warframe.com's own SPA has time to boot and make its
        // initialisation calls — we also intercept those via the fetch wrapper above.
        setTimeout(function () {
            probeUrls.forEach(function (url) {
                _origFetch(url, { credentials: 'include' })
                    .then(function (r) { return r.text(); })
                    .then(function (t) { tryExtract(t); })
                    .catch(function () {});
            });
        }, 1200);
    }
})();"#;

#[tauri::command]
pub async fn open_console_login(app: tauri::AppHandle) -> Result<(), String> {
    // Close any stale window from a previous attempt.
    if let Some(w) = app.get_webview_window("console-login") {
        let _ = w.close();
    }
    let url = tauri::Url::parse("https://www.warframe.com/login")
        .map_err(|e| e.to_string())?;
    // Use a dedicated data directory so this WebView never inherits an existing
    // warframe.com browser session. Without this, already-logged-in users get
    // redirected to the homepage and login.php is never called, so our
    // credential interceptor never fires.
    let data_dir = std::env::temp_dir().join("frameforge-console-login");
    tauri::WebviewWindowBuilder::new(&app, "console-login", tauri::WebviewUrl::External(url))
        .title("FrameForge — Warframe Login")
        .inner_size(500.0, 720.0)
        .resizable(true)
        .data_directory(data_dir)
        .initialization_script(INIT_SCRIPT)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Emits credentials to the main window and closes the login WebView.
/// Called by handle_ffauth when the injected JS delivers a valid login response.
pub fn emit_console_credentials(app: &tauri::AppHandle, id: String, nonce: String) {
    if id.is_empty() || nonce.is_empty() {
        return;
    }
    let _ = app.emit(
        "console-login-success",
        serde_json::json!({ "accountId": id, "nonce": nonce }),
    );
    if let Some(w) = app.get_webview_window("console-login") {
        let _ = w.close();
    }
}

/// Handler for the `ffauth://` custom URI scheme.
/// Receives GET ffauth://c?i=<percent-encoded accountId>&n=<percent-encoded nonce>
pub fn handle_ffauth(
    app: &tauri::AppHandle,
    req: &tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    let uri = req.uri().to_string();
    if let Some(query) = uri.split('?').nth(1) {
        let mut id = String::new();
        let mut nonce = String::new();
        for kv in query.split('&') {
            let mut parts = kv.splitn(2, '=');
            match (parts.next(), parts.next()) {
                (Some("i"), Some(v)) => id    = url_decode(v),
                (Some("n"), Some(v)) => nonce = url_decode(v),
                _ => {}
            }
        }
        emit_console_credentials(app, id, nonce);
    }
    tauri::http::Response::builder()
        .status(200)
        .header("Access-Control-Allow-Origin", "*")
        .body(Vec::<u8>::new())
        .unwrap()
}

fn url_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(hi), Some(lo)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((hi << 4 | lo) as char);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { ' ' } else { b[i] as char });
        i += 1;
    }
    out
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
