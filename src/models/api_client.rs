use core::fmt;

#[derive(Debug)]
pub struct GetRequestArgs {
    pub token: String,
    pub path: ApiPath,
}

#[derive(Debug)]
pub enum ApiPath {
    Projects(Option<String>),
}

impl fmt::Display for ApiPath {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ApiPath::Projects(p) => match p {
                Some(value) => write!(f, "projects/{}", value),
                None => write!(f, "projects"),
            },
        }
    }
}
