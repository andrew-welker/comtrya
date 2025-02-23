use super::User;
use super::UserVariant;
use crate::actions::Action;
use crate::contexts::Contexts;
use crate::manifests::Manifest;
use crate::steps::Step;
use std::ops::Deref;

pub type UserAdd = User;

impl Action for UserAdd {
    fn plan(&self, _manifest: &Manifest, _context: &Contexts) -> anyhow::Result<Vec<Step>> {
        let variant: UserVariant = self.into();
        let box_provider = variant.provider.clone().get_provider();
        let provider = box_provider.deref();

        let mut atoms: Vec<Step> = vec![];

        atoms.append(&mut provider.add_user(&variant));

        Ok(atoms)
    }
}
