// SPDX-License-Identifier: GPL-3.0-only

//! Each window is a process of its own, so windows find each other over the
//! session bus. Every window owns `dev.twright.CosmicTermAgents.Window<pid>`,
//! which a Flatpak may do without asking, being under its own id, and answers
//! there whether it shows a session, and brings it to the front or lets it go
//! when asked. Sessions are named by [`crate::agent::Session::descriptor`].

use std::sync::{LazyLock, Mutex};
use tokio::sync::mpsc;

const PREFIX: &str = "dev.twright.CosmicTermAgents.Window";
const PATH: &str = "/dev/twright/CosmicTermAgents/Window";
const INTERFACE: &str = "dev.twright.CosmicTermAgents.Window";

/// The sessions this window shows, kept up to date by the window
static OPEN: LazyLock<Mutex<Vec<String>>> = LazyLock::new(Mutex::default);

pub fn set_open(descriptors: Vec<String>) {
    let mut open = OPEN.lock().unwrap();
    if *open != descriptors {
        *open = descriptors;
    }
}

fn shows(descriptor: &str) -> bool {
    OPEN.lock().unwrap().iter().any(|d| d == descriptor)
}

/// What another window asks of this one
#[derive(Clone, Debug)]
pub enum Request {
    /// Bring the session's tab to the front, and the window with this
    /// activation token
    Focus(String, String),
    /// Close the session's tab, leaving the session running
    Release(String),
}

struct Window {
    requests: mpsc::UnboundedSender<Request>,
}

#[zbus::interface(name = "dev.twright.CosmicTermAgents.Window")]
impl Window {
    fn has_session(&self, descriptor: &str) -> bool {
        shows(descriptor)
    }

    fn focus_session(&self, descriptor: String, token: String) -> bool {
        shows(&descriptor) && self.requests.send(Request::Focus(descriptor, token)).is_ok()
    }

    fn release_session(&self, descriptor: String) -> bool {
        shows(&descriptor) && self.requests.send(Request::Release(descriptor)).is_ok()
    }
}

fn own_name() -> String {
    format!("{PREFIX}{}", std::process::id())
}

/// Answer other windows until the connection is dropped, passing on what
/// they ask
pub async fn serve(requests: mpsc::UnboundedSender<Request>) -> zbus::Result<zbus::Connection> {
    zbus::connection::Builder::session()?
        .name(own_name())?
        .serve_at(PATH, Window { requests })?
        .build()
        .await
}

async fn call(connection: &zbus::Connection, peer: &str, method: &str, args: &(impl serde::Serialize + zbus::zvariant::DynamicType)) -> zbus::Result<bool> {
    let proxy = zbus::Proxy::new(connection, peer.to_string(), PATH, INTERFACE).await?;
    proxy.call(method, args).await
}

/// The bus name of another window showing a session, if one does
pub async fn find(descriptor: String) -> Result<Option<String>, String> {
    let connection = zbus::Connection::session().await.map_err(|e| e.to_string())?;
    let bus = zbus::fdo::DBusProxy::new(&connection).await.map_err(|e| e.to_string())?;
    let own = own_name();
    for name in bus.list_names().await.map_err(|e| e.to_string())? {
        let name = name.as_str();
        if !name.starts_with(PREFIX) || name == own {
            continue;
        }
        // A window that does not answer, such as an older one, is skipped
        if call(&connection, name, "HasSession", &(descriptor.as_str(),)).await == Ok(true) {
            return Ok(Some(name.to_string()));
        }
    }
    Ok(None)
}

/// Ask a window to bring a session to the front, raising itself with token
pub async fn focus(peer: String, descriptor: String, token: String) -> Result<bool, String> {
    let connection = zbus::Connection::session().await.map_err(|e| e.to_string())?;
    call(&connection, &peer, "FocusSession", &(descriptor, token))
        .await
        .map_err(|e| e.to_string())
}

/// Ask a window to close its tab of a session
pub async fn release(peer: String, descriptor: String) -> Result<bool, String> {
    let connection = zbus::Connection::session().await.map_err(|e| e.to_string())?;
    call(&connection, &peer, "ReleaseSession", &(descriptor,))
        .await
        .map_err(|e| e.to_string())
}
