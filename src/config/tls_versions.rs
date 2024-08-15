use super::SetOpt;
use curl::easy::{Easy2, TlsVersion};

#[derive(Clone, Debug)]
pub(crate) struct TLSVersions {
    min_version: TlsVersion,
    max_version: TlsVersion
}

impl TLSVersions {
    pub fn new<T: AsRef<str>>(keys: &[T]) -> Self {
        TLSVersions(keys.into_iter().map(|x| x.as_ref()).collect::<Vec<_>>().join(";"))
    }
}

impl SetOpt for TLSVersions {
    fn set_opt<H>(&self, easy: &mut Easy2<H>) -> Result<(), curl::Error> {
        easy.ssl_min_version(self.min_version)?;
        easy.ssl_max_version(self.max_version)?;
    }
}
