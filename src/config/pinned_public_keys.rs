use super::SetOpt;
use curl::easy::Easy2;

#[derive(Clone, Debug)]
pub(crate) struct PinnedPublicKeys(String);

impl PinnedPublicKeys {
    pub fn new<T: AsRef<str>>(keys: &[T]) -> Self {
        PinnedPublicKeys(keys.into_iter().map(|x| x.as_ref()).collect::<Vec<_>>().join(";"))
    }
}

impl SetOpt for PinnedPublicKeys {
    fn set_opt<H>(&self, easy: &mut Easy2<H>) -> Result<(), curl::Error> {
        easy.pinned_public_key(&self.0)
    }
}
