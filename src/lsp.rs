use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(60);

// One synchronous JSON-RPC session; the reader thread bounds server waits.
pub struct Lsp {
    child: Child,
    input: ChildStdin,
    messages: Receiver<Result<Value>>,
    next_id: u64,
}

impl Lsp {
    pub fn start(root: &Path, uri: &str) -> Result<Self> {
        let executable =
            std::env::var_os("RUST_ANALYZER").unwrap_or_else(|| "rust-analyzer".into());
        let mut child = Command::new(executable)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("Cannot start rust-analyzer; install it or set RUST_ANALYZER")?;
        let input = child.stdin.take().context("Missing server stdin")?;
        let output = child.stdout.take().context("Missing server stdout")?;
        let (sender, messages) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let message = read_message(&mut reader);
                let failed = message.is_err();
                if sender.send(message).is_err() || failed {
                    break;
                }
            }
        });
        let mut client = Self {
            child,
            input,
            messages,
            next_id: 0,
        };
        let init = client.request(
            "initialize",
            json!({
                "processId": std::process::id(), "rootUri": uri,
                "capabilities": {
                    "textDocument": {
                        "documentSymbol": {"hierarchicalDocumentSymbolSupport": true},
                        "callHierarchy": {"dynamicRegistration": false}
                    },
                    "experimental": {"serverStatusNotification": true}
                },
                "initializationOptions": {
                    "checkOnSave": false,
                    "cargo": {"buildScripts": {"enable": false}},
                    "procMacro": {"enable": false}
                }
            }),
        )?;
        for capability in ["documentSymbolProvider", "callHierarchyProvider"] {
            ensure!(
                !init["capabilities"][capability].is_null()
                    && init["capabilities"][capability] != false,
                "rust-analyzer does not support {capability}; cannot extract this graph"
            );
        }
        client.notify("initialized", json!({}))?;
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let message = client.receive(deadline)?;
            client.handle_server_message(&message)?;
            if message["method"] == "experimental/serverStatus"
                && message["params"]["quiescent"] == true
            {
                ensure!(
                    message["params"]["health"] != "error",
                    "Workspace loading failed: {}",
                    message["params"]
                );
                break;
            }
        }
        Ok(client)
    }

    fn send(&mut self, message: Value) -> Result<()> {
        let body = serde_json::to_vec(&message)?;
        write!(self.input, "Content-Length: {}\r\n\r\n", body.len())?;
        self.input.write_all(&body)?;
        self.input.flush()?;
        Ok(())
    }

    pub fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}))
    }

    fn receive(&self, deadline: Instant) -> Result<Value> {
        self.messages
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .context("Timed out waiting for rust-analyzer (60 seconds), or server disconnected")?
    }

    fn handle_server_message(&mut self, message: &Value) -> Result<()> {
        if let Some(id) = message.get("id") {
            if message["method"] == "window/workDoneProgress/create" {
                self.send(json!({"jsonrpc": "2.0", "id": id, "result": null}))?;
            } else {
                self.send(json!({"jsonrpc": "2.0", "id": id,
                    "error": {"code": -32601, "message": "Unsupported client request"}}))?;
            }
        }
        if message["method"] == "experimental/serverStatus" && message["params"]["health"] != "ok" {
            eprintln!("rust-analyzer workspace status: {}", message["params"]);
        }
        Ok(())
    }

    pub fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let message = self
                .receive(deadline)
                .with_context(|| format!("LSP request {method}"))?;
            if message.get("method").is_some() {
                self.handle_server_message(&message)?;
            } else if message["id"] == id {
                if let Some(error) = message.get("error") {
                    bail!("LSP request {method} failed: {error}");
                }
                return message
                    .get("result")
                    .cloned()
                    .context("LSP response has no result");
            }
        }
    }

    pub fn shutdown(&mut self) -> Result<()> {
        self.request("shutdown", Value::Null)?;
        self.notify("exit", Value::Null)
    }
}

impl Drop for Lsp {
    fn drop(&mut self) {
        // Reap the server on errors too; never leave a background analyzer behind.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn read_message(reader: &mut impl BufRead) -> Result<Value> {
    let mut length = None;
    loop {
        let mut line = String::new();
        ensure!(
            reader.read_line(&mut line)? != 0,
            "rust-analyzer closed stdout; check its installation and stderr"
        );
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((key, value)) = line.split_once(':')
            && key.eq_ignore_ascii_case("Content-Length")
        {
            length = Some(value.trim().parse::<usize>()?);
        }
    }
    let length = length.context("LSP message has no Content-Length")?;
    ensure!(length <= 64 * 1024 * 1024, "LSP message exceeds 64 MiB");
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(serde_json::from_slice(&body)?)
}
