use crate::Error;
use crate::error::WeaverErrorKind;
use crate::error::{IoError, SerDeError};
use atrium_api::types::string::Did;
use atrium_common::store::Store;
use atrium_oauth::store::session::Session;
use atrium_oauth::store::state::InternalStateData;
use std::path::{Path, PathBuf};

pub struct SimpleJsonFileSessionStore<T = PathBuf>
where
    T: AsRef<Path>,
{
    path: T,
}

impl<T> SimpleJsonFileSessionStore<T>
where
    T: AsRef<Path>,
{
    pub fn new(path: T) -> Self {
        Self { path }
    }

    pub async fn get_session(&self, did: &Did) -> Result<Option<Session>, Error> {
        let path = self
            .path
            .as_ref()
            .join(format!("{}_session.json", did.as_str()));
        let file = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| Error::new(vec![WeaverErrorKind::from(IoError::from(e))]));
        let file = if let Err(e) = &file {
            println!("Failed to read session file: {e}");
            return Ok(None);
        } else {
            file?
        };

        let session = serde_json::from_str(&file)
            .map_err(|e| Error::new(vec![WeaverErrorKind::from(SerDeError::from(e))]))?;
        Ok(Some(session))
    }

    pub async fn set_session(&self, did: Did, session: Session) -> Result<(), Error> {
        let path = self
            .path
            .as_ref()
            .join(format!("{}_session.json", did.as_str()));
        let file = serde_json::to_string(&session)
            .map_err(|e| Error::new(vec![WeaverErrorKind::from(SerDeError::from(e))]))?;

        tokio::fs::write(path, file)
            .await
            .map_err(|e| Error::new(vec![WeaverErrorKind::from(IoError::from(e))]))?;

        Ok(())
    }

    pub async fn get_state(&self, key: &str) -> Result<Option<InternalStateData>, Error> {
        let path = self.path.as_ref().join(format!("{}_state.json", key));
        let file = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| Error::new(vec![WeaverErrorKind::from(IoError::from(e))]));
        let file = if let Err(e) = &file {
            println!("Failed to read state file: {e}");
            return Ok(None);
        } else {
            file?
        };
        let state = serde_json::from_str(&file)
            .map_err(|e| Error::new(vec![WeaverErrorKind::from(SerDeError::from(e))]))?;
        Ok(Some(state))
    }

    pub async fn set_state(&self, key: String, state: InternalStateData) -> Result<(), Error> {
        let path = self.path.as_ref().join(format!("{}_state.json", key));

        let file = serde_json::to_string(&state)
            .map_err(|e| Error::new(vec![WeaverErrorKind::from(SerDeError::from(e))]))?;

        tokio::fs::write(path, file)
            .await
            .map_err(|e| Error::new(vec![WeaverErrorKind::from(IoError::from(e))]))?;

        Ok(())
    }
}

impl Store<Did, Session> for SimpleJsonFileSessionStore<PathBuf> {
    type Error = crate::Error;

    async fn get(&self, key: &Did) -> Result<Option<Session>, Self::Error> {
        self.get_session(key).await
    }

    async fn set(&self, key: Did, value: Session) -> Result<(), Self::Error> {
        self.set_session(key, value).await
    }

    async fn del(&self, key: &Did) -> Result<(), Self::Error> {
        let path = self.path.join(format!("{}_session.json", key.as_str()));
        tokio::fs::remove_file(path)
            .await
            .map_err(|e| Error::new(vec![WeaverErrorKind::from(IoError::from(e))]))
    }

    async fn clear(&self) -> Result<(), Self::Error> {
        let mut files = tokio::fs::read_dir(&self.path)
            .await
            .map_err(|e| Error::new(vec![WeaverErrorKind::from(IoError::from(e))]))?;
        while let Some(entry) = files
            .next_entry()
            .await
            .map_err(|e| Error::new(vec![WeaverErrorKind::from(IoError::from(e))]))?
        {
            let file_name = entry.file_name();
            if let Some(file_name) = file_name.to_str() {
                if file_name.ends_with("_session.json") {
                    tokio::fs::remove_file(entry.path())
                        .await
                        .map_err(|e| Error::new(vec![WeaverErrorKind::from(IoError::from(e))]))?;
                }
            }
        }
        Ok(())
    }
}

impl Store<String, InternalStateData> for SimpleJsonFileSessionStore<PathBuf> {
    type Error = crate::Error;

    async fn get(&self, key: &String) -> Result<Option<InternalStateData>, Self::Error> {
        self.get_state(key.as_str()).await
    }

    async fn set(&self, key: String, value: InternalStateData) -> Result<(), Self::Error> {
        self.set_state(key, value).await
    }

    async fn del(&self, key: &String) -> Result<(), Self::Error> {
        let path = self.path.join(format!("{}_state.json", key.as_str()));
        tokio::fs::remove_file(path)
            .await
            .map_err(|e| Error::new(vec![WeaverErrorKind::from(IoError::from(e))]))
    }

    async fn clear(&self) -> Result<(), Self::Error> {
        let mut files = tokio::fs::read_dir(&self.path)
            .await
            .map_err(|e| Error::new(vec![WeaverErrorKind::from(IoError::from(e))]))?;
        while let Some(entry) = files
            .next_entry()
            .await
            .map_err(|e| Error::new(vec![WeaverErrorKind::from(IoError::from(e))]))?
        {
            let file_name = &entry.file_name();
            if let Some(file_name) = file_name.to_str() {
                if file_name.ends_with("_state.json") {
                    tokio::fs::remove_file(entry.path())
                        .await
                        .map_err(|e| Error::new(vec![WeaverErrorKind::from(IoError::from(e))]))?;
                }
            }
        }
        Ok(())
    }
}

impl atrium_oauth::store::session::SessionStore for SimpleJsonFileSessionStore<PathBuf> {}

impl atrium_oauth::store::state::StateStore for SimpleJsonFileSessionStore<PathBuf> {}

impl Default for SimpleJsonFileSessionStore<PathBuf> {
    fn default() -> Self {
        let path = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("./"))
            .join("weaver/sessions");
        Self { path }
    }
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("./"))
        .join("weaver/sessions")
}
