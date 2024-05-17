use std::{
    any::{Any, TypeId},
    collections::HashMap,
    hash::{BuildHasherDefault, Hasher},
};

use axum::BoxError;
use once_cell::sync::Lazy;
use parking_lot::Mutex;

type AnyMap = HashMap<TypeId, Box<dyn StateClone + Send>, BuildHasherDefault<IdHasher>>;

static STATES: Lazy<Mutex<AnyMap>> = Lazy::new(|| Mutex::new(Default::default()));

/// Hasher for [`TypeId`] values
///
/// No transformations are necessary, as type IDs are already pre-hashed by compiler.
#[derive(Default)]
struct IdHasher(u64);

impl Hasher for IdHasher {
    fn write(&mut self, _: &[u8]) {
        unreachable!("TypeId calls write_u64");
    }

    #[inline]
    fn write_u64(&mut self, id: u64) {
        self.0 = id;
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}

///
pub fn get<S>() -> S
where
    S: AutoState + StateClone + Clone + Default + Send,
{
    let type_id = TypeId::of::<S>();

    // SAFETY: It is reasonable to assume that entry keyed as type ID of S
    // will have type S. So unwrap() cannot panic here.
    STATES
        .lock()
        .entry(type_id)
        .or_insert_with(|| Box::new(S::new_default()))
        .as_any()
        .downcast_ref::<S>()
        .unwrap()
        .clone()
}

///
pub trait AutoState: StateClone {
    ///
    fn new_default() -> Self
    where
        Self: Sized + Default,
    {
        // FIXME: remove requirement for state types to implement Default
        Self::default()
    }

    ///
    fn init(&mut self) -> Result<(), BoxError> {
        Ok(())
    }
}

///
pub trait StateClone: Any {
    ///
    fn clone_box(&self) -> Box<dyn StateClone + Send>;
    ///
    fn as_any(&self) -> &dyn Any;
    ///
    fn as_any_mut(&mut self) -> &mut dyn Any;
    ///
    fn into_any(self: Box<Self>) -> Box<dyn Any>;
}

impl<T: Clone + Send + Sync + 'static> StateClone for T {
    fn clone_box(&self) -> Box<dyn StateClone + Send> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

impl Clone for Box<dyn StateClone + Send + 'static> {
    fn clone(&self) -> Self {
        (**self).clone_box()
    }
}
