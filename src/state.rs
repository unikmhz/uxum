//! State extractor support for method handlers

use std::{
    any::{Any, TypeId},
    collections::HashMap,
    hash::{BuildHasherDefault, Hasher},
};

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

/// Get a clone of previously registered state object
///
/// # Panics
///
/// This call will panic if no previously registered object of type S has been found.
pub fn get<S>() -> S
where
    S: StateClone + Clone + Send,
{
    let type_id = TypeId::of::<S>();
    match STATES.lock().get(&type_id) {
        // SAFETY: It is reasonable to assume that entry keyed as type ID of S
        // will have type S. So unwrap() cannot panic here.
        Some(state) => (**state).as_any().downcast_ref::<S>().unwrap().clone(),
        None => panic!("State is missing from state registry"),
    }
}

/// Register state object for use in handlers
pub fn put<S>(state: S)
where
    S: StateClone + Clone + Send,
{
    let type_id = TypeId::of::<S>();
    STATES.lock().insert(type_id, Box::new(state));
}

/// Trait that is required to be implemented for types of state objects
pub trait StateClone: Any {
    /// Auto-boxing clone helper method
    fn clone_box(&self) -> Box<dyn StateClone + Send>;
    /// Convert to `&dyn Any`
    fn as_any(&self) -> &dyn Any;
    /// Convert to `&mut dyn Any`
    fn as_any_mut(&mut self) -> &mut dyn Any;
    /// Convert to `Box<dyn Any>`
    fn into_any(self: Box<Self>) -> Box<dyn Any>;
}

impl<T> StateClone for T
where
    T: Clone + Send + 'static,
{
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
