// SPDX-License-Identifier: GPL-3.0-only

//! Finding other computers to add as machines, and checking one.
//!
//! Candidates come from Tailscale's peers and from `~/.ssh/config`, merged
//! when they are the same computer: an ssh alias is preferred, since it
//! carries the user name and any other options ssh needs. Checking a machine
//! logs in once and reads its user name, host name, home and registry id.

use crate::config::Machine;

/// A computer that could be added
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Candidate {
    pub name: String,
    /// What ssh would connect to
    pub host: String,
    pub user: String,
    pub sources: Vec<Source>,
    /// From Tailscale, whether it is connected now
    pub online: Option<bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    Tailscale,
    SshConfig,
}

impl Candidate {
    pub fn machine(&self) -> Machine {
        Machine {
            name: self.name.clone(),
            host: self.host.clone(),
            user: self.user.clone(),
            ..Machine::default()
        }
    }
}

/// A `Host` entry of an ssh config
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SshHost {
    /// Its names, the first being the one to use
    pub aliases: Vec<String>,
    pub hostname: Option<String>,
    pub user: Option<String>,
}

/// The concrete `Host` entries of an ssh config: no patterns, no `Match`
pub fn parse_ssh_config(text: &str) -> Vec<SshHost> {
    let mut hosts: Vec<SshHost> = Vec::new();
    let mut current: Option<SshHost> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = match line.split_once(|c: char| c.is_whitespace() || c == '=') {
            Some((key, value)) => (
                key.to_ascii_lowercase(),
                value.trim().trim_start_matches('=').trim(),
            ),
            None => continue,
        };
        match key.as_str() {
            "host" | "match" => {
                hosts.extend(current.take());
                if key == "host" {
                    let aliases: Vec<String> = value
                        .split_whitespace()
                        .filter(|a| !a.contains(['*', '?', '!']))
                        .map(str::to_string)
                        .collect();
                    if !aliases.is_empty() {
                        current = Some(SshHost {
                            aliases,
                            ..SshHost::default()
                        });
                    }
                }
            }
            "hostname" => {
                if let Some(host) = &mut current {
                    host.hostname.get_or_insert_with(|| value.to_string());
                }
            }
            "user" => {
                if let Some(host) = &mut current {
                    host.user.get_or_insert_with(|| value.to_string());
                }
            }
            _ => {}
        }
    }
    hosts.extend(current);
    hosts
}

/// A peer, or this computer, as `tailscale status --json` lists it
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TailscalePeer {
    pub host_name: String,
    /// The MagicDNS name, without its final dot
    pub dns_name: String,
    pub os: String,
    pub online: bool,
}

/// This computer and its peers, from `tailscale status --json`
pub fn parse_tailscale(json: &str) -> Option<(TailscalePeer, Vec<TailscalePeer>)> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let peer = |p: &serde_json::Value| TailscalePeer {
        host_name: p["HostName"].as_str().unwrap_or_default().to_string(),
        dns_name: p["DNSName"]
            .as_str()
            .unwrap_or_default()
            .trim_end_matches('.')
            .to_string(),
        os: p["OS"].as_str().unwrap_or_default().to_string(),
        online: p["Online"].as_bool().unwrap_or(false),
    };
    let me = peer(&value["Self"]);
    let mut peers: Vec<TailscalePeer> = value["Peer"]
        .as_object()
        .map(|peers| peers.values().map(peer).collect())
        .unwrap_or_default();
    peers.sort_by(|a, b| a.host_name.cmp(&b.host_name));
    Some((me, peers))
}

/// Hosts that are code forges, not computers to work on
fn is_forge(host: &SshHost) -> bool {
    host.user.as_deref() == Some("git")
        || host.aliases.iter().chain(&host.hostname).any(|name| {
            ["github", "gitlab", "bitbucket", "codeberg"]
                .iter()
                .any(|f| name.contains(f))
        })
}

/// Computers worth offering, best first: from both sources, then the ssh
/// config, then Tailscale, leaving out this computer, forges, phones and
/// machines already added
pub fn candidates(
    ssh: &[SshHost],
    tailscale: Option<&(TailscalePeer, Vec<TailscalePeer>)>,
    known: &[Machine],
) -> Vec<Candidate> {
    let mut found: Vec<Candidate> = Vec::new();
    let me = tailscale.map(|(me, _)| me);
    let is_me = |name: &str| me.is_some_and(|me| name == me.host_name || name == me.dns_name);
    for host in ssh.iter().filter(|h| !is_forge(h)) {
        let target = host
            .hostname
            .clone()
            .unwrap_or_else(|| host.aliases[0].clone());
        if host.aliases.iter().any(|a| is_me(a)) || is_me(&target) {
            continue;
        }
        found.push(Candidate {
            name: host.aliases[0].clone(),
            host: host.aliases[0].clone(),
            user: host.user.clone().unwrap_or_default(),
            sources: vec![Source::SshConfig],
            online: None,
        });
    }
    if let Some((_, peers)) = tailscale {
        for peer in peers {
            if matches!(peer.os.as_str(), "android" | "iOS" | "ios") {
                continue;
            }
            let short = peer.dns_name.split('.').next().unwrap_or_default();
            // The same computer, if an ssh alias names it or points at it
            let same = ssh.iter().position(|h| {
                h.aliases
                    .iter()
                    .chain(&h.hostname)
                    .any(|name| name == &peer.host_name || name == &peer.dns_name || name == short)
            });
            match same.and_then(|i| found.iter_mut().find(|c| c.host == ssh[i].aliases[0])) {
                Some(candidate) => {
                    candidate.sources.push(Source::Tailscale);
                    candidate.online = Some(peer.online);
                }
                None if same.is_none() => found.push(Candidate {
                    name: peer.host_name.clone(),
                    host: peer.dns_name.clone(),
                    user: String::new(),
                    sources: vec![Source::Tailscale],
                    online: Some(peer.online),
                }),
                None => {}
            }
        }
    }
    found.retain(|c| {
        !known.iter().any(|m| {
            m.host == c.host
                || m.destination() == c.machine().destination()
                || (!m.hostname.is_empty() && m.hostname == c.name)
        })
    });
    found.sort_by_key(|c| {
        (
            std::cmp::Reverse(c.sources.len()),
            !c.sources.contains(&Source::SshConfig),
            c.online == Some(false),
        )
    });
    found
}

/// What checking a machine found
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Probe {
    pub user: String,
    pub hostname: String,
    pub home: String,
    pub host_id: String,
}

/// The script that checks a machine; its output is read by [`parse_probe`]
pub const PROBE_SCRIPT: &str = "id -un\nhostname\necho \"$HOME\"\n\
     cat \"${XDG_STATE_HOME:-$HOME/.local/state}/cosmic-term/projects.json\" 2>/dev/null || true\n";

pub fn parse_probe(output: &str) -> Option<Probe> {
    let mut lines = output.lines();
    let user = lines.next()?.trim().to_string();
    let hostname = lines.next()?.trim().to_string();
    let home = lines.next()?.trim().to_string();
    let registry: String = lines.collect::<Vec<_>>().join("\n");
    let host_id = serde_json::from_str::<serde_json::Value>(&registry)
        .ok()
        .and_then(|v| v["host_id"].as_str().map(str::to_string))
        .unwrap_or_default();
    (!user.is_empty()).then_some(Probe {
        user,
        hostname,
        home,
        host_id,
    })
}

/// Why ssh failed, in words that say what to do about it
pub fn explain_ssh_error(destination: &str, stderr: &str) -> String {
    let last = stderr.trim().lines().last().unwrap_or("ssh failed").trim();
    if stderr.contains("Host key verification failed")
        || stderr.contains("REMOTE HOST IDENTIFICATION HAS CHANGED")
    {
        format!(
            "ssh does not trust this computer's host key yet. Connect once in a terminal with `ssh {destination}` to check and accept it, then check again."
        )
    } else if stderr.contains("Permission denied") {
        format!(
            "ssh could not log in to {destination} ({last}). Check the user name, and that your key is accepted there."
        )
    } else if stderr.contains("Could not resolve hostname") {
        format!("{destination} could not be found ({last}).")
    } else if stderr.contains("timed out")
        || stderr.contains("No route to host")
        || stderr.contains("Connection refused")
    {
        format!("{destination} could not be reached ({last}).")
    } else {
        last.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SSH_CONFIG: &str = "\
# comment
Host github.com
Host github-household
  HostName github.com
  User git
Host gitlab.au.dk
Host au-laptop d59500
  HostName d59500.tail57c02a.ts.net
  User au759518
Host trinity
  HostName 192.168.0.153
  User twright
Host *.example.org
  User nobody
Match host foo
  User bar
Host neo
  User twright
";

    fn tailscale() -> (TailscalePeer, Vec<TailscalePeer>) {
        let json = r#"{"Self":{"HostName":"neo","DNSName":"neo.tail57c02a.ts.net.","OS":"linux","Online":true},
            "Peer":{"a":{"HostName":"d59500","DNSName":"d59500.tail57c02a.ts.net.","OS":"linux","Online":true},
                    "b":{"HostName":"loki","DNSName":"loki.tail57c02a.ts.net.","OS":"linux","Online":true},
                    "c":{"HostName":"Pixel 7a","DNSName":"pixel-7a.tail57c02a.ts.net.","OS":"android","Online":true},
                    "d":{"HostName":"trinity","DNSName":"trinity.tail57c02a.ts.net.","OS":"linux","Online":false}}}"#;
        parse_tailscale(json).unwrap()
    }

    #[test]
    fn ssh_config_hosts_skip_patterns_and_matches() {
        let hosts = parse_ssh_config(SSH_CONFIG);
        let names: Vec<&str> = hosts.iter().map(|h| h.aliases[0].as_str()).collect();
        assert_eq!(
            names,
            [
                "github.com",
                "github-household",
                "gitlab.au.dk",
                "au-laptop",
                "trinity",
                "neo"
            ]
        );
        assert_eq!(hosts[3].aliases, ["au-laptop", "d59500"]);
        assert_eq!(hosts[3].user.as_deref(), Some("au759518"));
        assert_eq!(hosts[5].user.as_deref(), Some("twright"));
        assert_eq!(hosts[5].hostname, None);
    }

    #[test]
    fn candidates_merge_sources_and_leave_out_this_computer() {
        let found = candidates(&parse_ssh_config(SSH_CONFIG), Some(&tailscale()), &[]);
        let summary: Vec<(&str, &str, &str, usize, Option<bool>)> = found
            .iter()
            .map(|c| {
                (
                    c.name.as_str(),
                    c.host.as_str(),
                    c.user.as_str(),
                    c.sources.len(),
                    c.online,
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("au-laptop", "au-laptop", "au759518", 2, Some(true)),
                ("trinity", "trinity", "twright", 2, Some(false)),
                ("loki", "loki.tail57c02a.ts.net", "", 1, Some(true)),
            ]
        );
    }

    #[test]
    fn known_machines_are_not_offered_again() {
        let known = [Machine {
            name: "laptop".into(),
            host: "au-laptop".into(),
            ..Machine::default()
        }];
        let found = candidates(&parse_ssh_config(SSH_CONFIG), Some(&tailscale()), &known);
        assert!(found.iter().all(|c| c.host != "au-laptop"));
        // Without Tailscale, the ssh config alone
        let found = candidates(&parse_ssh_config(SSH_CONFIG), None, &[]);
        assert_eq!(
            found.iter().map(|c| c.host.as_str()).collect::<Vec<_>>(),
            ["au-laptop", "trinity", "neo"]
        );
    }

    #[test]
    fn probes_parse_with_and_without_a_registry() {
        let probe = parse_probe(
            "au759518\nd59500\n/home/au759518\n{\n  \"schema\": 1,\n  \"host_id\": \"b42f\"\n}\n",
        )
        .unwrap();
        assert_eq!(
            probe,
            Probe {
                user: "au759518".into(),
                hostname: "d59500".into(),
                home: "/home/au759518".into(),
                host_id: "b42f".into(),
            }
        );
        assert_eq!(parse_probe("u\nh\n/home/u\n").unwrap().host_id, "");
        assert_eq!(parse_probe(""), None);
    }

    #[test]
    fn ssh_errors_say_what_to_do() {
        assert!(
            explain_ssh_error("twright@neo", "Host key verification failed.\n")
                .contains("ssh twright@neo")
        );
        assert!(
            explain_ssh_error("x", "x@y: Permission denied (publickey).").contains("user name")
        );
    }
}
