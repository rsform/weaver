use jacquard::{CowStr, IntoStatic, types::string::Did};

#[derive(Clone, Debug, PartialEq)]
pub struct AuthState {
    pub did: Option<Did<'static>>,
    pub session_id: Option<CowStr<'static>>,
}

impl Default for AuthState {
    fn default() -> Self {
        Self {
            did: None,
            session_id: None,
        }
    }
}

impl AuthState {
    pub fn is_authenticated(&self) -> bool {
        self.did.is_some()
    }

    pub fn set_authenticated(&mut self, did: Did<'_>, session_id: CowStr<'_>) {
        self.did = Some(did.into_static());
        self.session_id = Some(session_id.into_static());
    }

    pub fn clear(&mut self) {
        self.did = None;
        self.session_id = None;
    }
}
