// SPDX-License-Identifier: GPL-3.0-only

//! Host requirements for opening another computer's files. Package commands
//! are selected from known distributions, never assembled from probe output.

/// Run on the host (also when the UI is a Flatpak). No Python is needed to
/// explain missing tools. These are the programs used by ct-mount itself.
pub const PROBE: &str = r#"printf 'ct-mount-setup-v1\n'
ID= ID_LIKE= PRETTY_NAME=
if [ -r /etc/os-release ]; then . /etc/os-release; fi
printf 'id=%s\nlike=%s\nname=%s\n' "$ID" "$ID_LIKE" "$PRETTY_NAME"
if [ -e /run/ostree-booted ]; then echo atomic=yes; else echo atomic=no; fi
if [ -r /dev/fuse ] && [ -w /dev/fuse ]; then echo fuse=yes; else echo fuse=no; fi
for tool in sshfs ssh flock setsid mountpoint timeout; do
    command -v "$tool" >/dev/null 2>&1 || printf 'missing=%s\n' "$tool"
done
command -v fusermount3 >/dev/null 2>&1 || command -v fusermount >/dev/null 2>&1 || echo missing=fusermount
if [ -e /run/ostree-booted ] && command -v rpm-ostree >/dev/null 2>&1; then
    printf 'ostree-status:\n'
    rpm-ostree status --json 2>/dev/null || true
fi
"#;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Status {
    pub system: String,
    pub atomic: bool,
    pub fuse_access: bool,
    pub reboot_pending: bool,
    pub missing: Vec<String>,
    distribution: Distribution,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Distribution {
    Fedora,
    Debian,
    Other,
}

impl Status {
    pub fn parse(output: &str) -> Result<Self, String> {
        let (_, output) = output
            .split_once("ct-mount-setup-v1\n")
            .ok_or("the computer did not return a remote-files check")?;
        let (output, ostree) = output
            .split_once("ostree-status:\n")
            .map_or((output, None), |(check, json)| (check, Some(json)));
        let value = |key: &str| output.lines().find_map(|line| line.strip_prefix(key));
        let id = value("id=").ok_or("missing operating system in remote-files check")?;
        let like = value("like=").unwrap_or_default();
        let belongs = |family| id == family || like.split_whitespace().any(|name| name == family);
        let distribution = if belongs("fedora") {
            Distribution::Fedora
        } else if belongs("debian") || belongs("ubuntu") || id == "pop" {
            Distribution::Debian
        } else {
            Distribution::Other
        };
        let boolean = |key| match value(key) {
            Some("yes") => Ok(true),
            Some("no") => Ok(false),
            _ => Err("incomplete remote-files check".to_string()),
        };
        let mut missing: Vec<_> = output
            .lines()
            .filter_map(|line| line.strip_prefix("missing="))
            .map(str::to_string)
            .collect();
        missing.sort();
        missing.dedup();
        let mut status = Self {
            system: value("name=")
                .filter(|name| !name.is_empty())
                .unwrap_or(id)
                .to_string(),
            atomic: boolean("atomic=")?,
            fuse_access: boolean("fuse=")?,
            reboot_pending: false,
            missing,
            distribution,
        };
        if status.atomic {
            if let (Some(command), Some(json)) = (status.install_command(), ostree) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(json) {
                    if let Some(next) = json["deployments"]
                        .as_array()
                        .and_then(|deployments| deployments.first())
                    {
                        let requested = next["requested-packages"].as_array();
                        status.reboot_pending = next["staged"] == true
                            && next["booted"] == false
                            && command.split_whitespace().skip(3).all(|package| {
                                requested.is_some_and(|packages| {
                                    packages.iter().any(|p| p.as_str() == Some(package))
                                })
                            });
                    }
                }
            }
        }
        Ok(status)
    }

    pub fn ready(&self) -> bool {
        self.missing.is_empty() && self.fuse_access
    }

    /// The command shown to the user and run only by the install button.
    /// Unknown systems and requirements get manual guidance instead.
    pub fn install_command(&self) -> Option<String> {
        if self.reboot_pending {
            return None;
        }
        let manager = match (self.distribution, self.atomic) {
            (Distribution::Fedora, true) => "sudo rpm-ostree install",
            (Distribution::Fedora, false) => "sudo dnf install",
            (Distribution::Debian, false) => "sudo apt install",
            _ => return None,
        };
        let mut packages = std::collections::BTreeSet::new();
        for tool in &self.missing {
            packages.insert(match tool.as_str() {
                "sshfs" if self.distribution == Distribution::Fedora => "fuse-sshfs",
                "sshfs" => "sshfs",
                "fusermount" => "fuse3",
                "ssh" if self.distribution == Distribution::Fedora => "openssh-clients",
                "ssh" => "openssh-client",
                "flock" | "setsid" | "mountpoint" => "util-linux",
                "timeout" => "coreutils",
                _ => return None,
            });
        }
        // Installing SSHFS supplies its FUSE unmount dependency too.
        if self.missing.iter().any(|tool| tool == "sshfs") {
            packages.remove("fuse3");
        }
        (!packages.is_empty()).then(|| {
            format!(
                "{manager} {}",
                packages.into_iter().collect::<Vec<_>>().join(" ")
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(id: &str, like: &str, atomic: bool, fuse: bool, missing: &[&str]) -> Status {
        Status::parse(&format!(
            "ct-mount-setup-v1\nid={id}\nlike={like}\nname=Test OS\natomic={}\nfuse={}\n{}",
            if atomic { "yes" } else { "no" },
            if fuse { "yes" } else { "no" },
            missing
                .iter()
                .map(|tool| format!("missing={tool}\n"))
                .collect::<String>()
        ))
        .unwrap()
    }

    #[test]
    fn atomic_install_uses_the_host_deployment_and_requires_a_reboot() {
        let status = probe("fedora", "", true, true, &["sshfs", "fusermount"]);
        assert!(status.atomic);
        assert!(!status.ready());
        assert_eq!(
            status.install_command().as_deref(),
            Some("sudo rpm-ostree install fuse-sshfs")
        );
        let after_reboot = probe("fedora", "", true, true, &[]);
        assert!(after_reboot.ready());
        assert_eq!(after_reboot.install_command(), None);
    }

    #[test]
    fn regular_fedora_and_debian_family_get_their_own_packages() {
        assert_eq!(
            probe("fedora", "", false, true, &["sshfs"])
                .install_command()
                .as_deref(),
            Some("sudo dnf install fuse-sshfs")
        );
        assert_eq!(
            probe("pop", "ubuntu debian", false, true, &["sshfs"])
                .install_command()
                .as_deref(),
            Some("sudo apt install sshfs")
        );
        assert_eq!(
            probe("ubuntu", "debian", false, true, &["fusermount", "ssh"])
                .install_command()
                .as_deref(),
            Some("sudo apt install fuse3 openssh-client")
        );
    }

    #[test]
    fn missing_fuse_access_and_unknown_systems_are_not_reported_ready() {
        let status = probe("fedora", "", true, false, &[]);
        assert!(!status.ready());
        assert_eq!(status.install_command(), None);
        assert_eq!(
            probe("arch", "", false, true, &["sshfs"]).install_command(),
            None
        );
        assert_eq!(
            probe("debian", "", true, true, &["sshfs"]).install_command(),
            None
        );
    }

    #[test]
    fn a_staged_sshfs_install_offers_a_reboot_instead_of_installing_twice() {
        let status = Status::parse("ct-mount-setup-v1\nid=fedora\natomic=yes\nfuse=yes\nmissing=sshfs\nostree-status:\n{\"deployments\":[{\"staged\":true,\"booted\":false,\"requested-packages\":[\"fuse-sshfs\"]}]}").unwrap();
        assert!(status.reboot_pending);
        assert!(!status.ready());
        assert_eq!(status.install_command(), None);
    }

    #[test]
    fn an_old_layered_deployment_is_not_a_pending_install() {
        let status = Status::parse("ct-mount-setup-v1\nid=fedora\natomic=yes\nfuse=yes\nmissing=sshfs\nostree-status:\n{\"deployments\":[{\"staged\":false,\"booted\":true,\"requested-packages\":[]},{\"staged\":false,\"booted\":false,\"requested-packages\":[\"fuse-sshfs\"]}]}").unwrap();
        assert!(!status.reboot_pending);
        assert_eq!(
            status.install_command().as_deref(),
            Some("sudo rpm-ostree install fuse-sshfs")
        );
    }

    #[test]
    fn malformed_checks_and_unknown_tools_never_produce_an_install_command() {
        assert!(Status::parse("").is_err());
        assert!(Status::parse("ct-mount-setup-v1\nid=fedora\natomic=yes\n").is_err());
        let status = probe("fedora", "", true, true, &["sshfs; touch /tmp/wrong"]);
        assert_eq!(status.install_command(), None);
        assert!(!status.ready());
    }
}
