use cosmic::iced::clipboard::mime::AllowedMimeTypes;
use std::{borrow::Cow, error::Error, path::PathBuf, str};
use url::Url;

use crate::agent::Session;

/// The type of a dragged tab, whose data is its session's descriptor
pub const TAB_MIME: &str = "x-cosmic-term/tab";

/// Files, or a tab from this or another window
#[derive(Clone, Debug)]
pub struct DndDrop {
    pub paths: Vec<PathBuf>,
    pub session: Option<Session>,
}

impl AllowedMimeTypes for DndDrop {
    fn allowed() -> Cow<'static, [String]> {
        Cow::from(vec![
            TAB_MIME.to_string(),
            "x-special/gnome-copied-files".to_string(),
            "text/uri-list".to_string(),
        ])
    }
}

impl TryFrom<(Vec<u8>, String)> for DndDrop {
    type Error = Box<dyn Error>;
    fn try_from(value: (Vec<u8>, String)) -> Result<Self, Self::Error> {
        let (data, mime) = value;
        let mut paths = Vec::new();
        match mime.as_str() {
            TAB_MIME => {
                let session = Session::from_descriptor(str::from_utf8(&data)?)
                    .ok_or("the tab has no session")?;
                return Ok(Self {
                    paths,
                    session: Some(session),
                });
            }
            "text/uri-list" => {
                let text = str::from_utf8(&data)?;
                for line in text.lines() {
                    let url = Url::parse(line)?;
                    match url.to_file_path() {
                        Ok(path) => paths.push(path),
                        Err(()) => Err(format!("invalid file URL {:?}", url))?,
                    }
                }
            }
            "x-special/gnome-copied-files" => {
                let text = str::from_utf8(&data)?;
                for (i, line) in text.lines().enumerate() {
                    if i != 0 {
                        let url = Url::parse(line)?;
                        match url.to_file_path() {
                            Ok(path) => paths.push(path),
                            Err(()) => Err(format!("invalid file URL {:?}", url))?,
                        }
                    }
                }
            }
            _ => Err(format!("unsupported mime type {:?}", mime))?,
        }
        Ok(Self {
            paths,
            session: None,
        })
    }
}
