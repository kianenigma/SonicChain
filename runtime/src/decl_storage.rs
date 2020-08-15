/// Create a storage map struct.
#[macro_export]
macro_rules! decl_storage_map {
	($name:ident, $storage_name:expr, $key_type:ty, $value_type:ty) => {
		pub struct $name<R>(std::marker::PhantomData<R>);

		impl<R: $crate::ModuleRuntime> $name<R> {
			pub fn key_for(key: $key_type) -> $crate::primitives::Key {
				let mut final_key = Vec::new();
				let storage_name = format!("{}:{}", MODULE, $storage_name);
				log::trace!(target: "storage", "storage op @ [{}({:?})]", storage_name, key);
				final_key.extend(storage_name.as_bytes());
				final_key.extend(key.encode());
				final_key.into()
			}

			pub fn write(
				runtime: &R,
				key: $key_type,
				val: $value_type,
			) -> Result<(), $crate::primitives::ThreadId> {
				let encoded_value = val.encode();
				let final_key = Self::key_for(key);
				log::trace!(target: "storage", "write @ [{:?}]", val);
				runtime.write(&final_key, encoded_value.into())
			}

			pub fn read(
				runtime: &R,
				key: $key_type,
			) -> Result<$value_type, $crate::primitives::ThreadId> {
				let final_key = Self::key_for(key);
				let encoded = runtime.read(&final_key)?;
				let maybe_decoded = <$value_type as Decode>::decode(&mut &*encoded.0);
				log::trace!(target: "storage", "read [{:?}]", maybe_decoded);
				Ok(maybe_decoded.unwrap_or_default())
			}

			pub fn mutate(
				runtime: &R,
				key: $key_type,
				update: impl Fn(&mut $value_type) -> (),
			) -> Result<(), $crate::primitives::ThreadId> {
				let mut old = Self::read(runtime, key.clone())?;
				update(&mut old);
				Self::write(runtime, key, old)
			}

			pub fn clear(runtime: &R, key: $key_type) -> Result<(), $crate::primitives::ThreadId> {
				Self::write(runtime, key, Default::default())
			}

			pub fn exists(
				runtime: &R,
				key: $key_type,
			) -> Result<bool, $crate::primitives::ThreadId> {
				Self::read(runtime, key).map(|val| val != <$value_type as Default>::default())
			}
		}
	};
}

/// Create a storage value struct.
#[macro_export]
macro_rules! decl_storage_value {
	($name:ident, $storage_name:expr, $value_type:ty) => {
		pub struct $name<R>(std::marker::PhantomData<R>);

		impl<R: $crate::ModuleRuntime> $name<R> {
			pub fn key() -> $crate::primitives::Key {
				format!("{}:{}", MODULE, $storage_name)
					.as_bytes()
					.to_vec()
					.into()
			}

			pub fn write(
				runtime: &R,
				val: $value_type,
			) -> Result<(), $crate::primitives::ThreadId> {
				let encoded_value = val.encode();
				let final_key = Self::key();
				runtime.write(&final_key, encoded_value.into())
			}

			pub fn read(runtime: &R) -> Result<$value_type, $crate::primitives::ThreadId> {
				let final_key = Self::key();
				let encoded = runtime.read(&final_key)?;
				let maybe_decoded = <$value_type as Decode>::decode(&mut &*encoded.0);
				Ok(maybe_decoded.unwrap_or_default())
			}

			pub fn mutate(
				runtime: &R,
				update: impl Fn(&mut $value_type) -> (),
			) -> Result<(), $crate::primitives::ThreadId> {
				let mut old = Self::read(runtime)?;
				update(&mut old);
				Self::write(runtime, old)
			}

			pub fn clear(runtime: &R) -> Result<(), $crate::primitives::ThreadId> {
				Self::write(runtime, Default::default())
			}
		}
	};
}

#[cfg(test)]
#[allow(dead_code)]
mod tests {
	use crate::{RuntimeState, WorkerRuntime};
	use parity_scale_codec::{Decode, Encode};
	use primitives::testing;
	use primitives::AccountId;

	const MODULE: &'static str = "test";

	#[derive(Encode, Decode, Debug, Clone, Default, Eq, PartialEq)]
	pub struct Something(pub u32);

	decl_storage_map!(TestMap, "map", u8, Something);
	decl_storage_map!(AccMap, "accMap", AccountId, AccountId);
	decl_storage_value!(TestValue, "value", Vec<u32>);

	#[test]
	fn map_account_id_default_works() {
		let state = RuntimeState::new().as_arc();
		let rt = WorkerRuntime::new(state, 0);

		assert_eq!(
			AccMap::read(&rt, testing::alice().public()).unwrap(),
			testing::default().public()
		);
	}

	#[test]
	fn map_works() {
		let state = RuntimeState::new().as_arc();
		let rt = WorkerRuntime::new(state, 0);

		// reads default
		assert_eq!(TestMap::read(&rt, 10), Ok(Something(0)));

		assert_eq!(TestMap::exists(&rt, 19).unwrap(), false);

		// can write
		assert_eq!(TestMap::write(&rt, 10, Something(10)), Ok(()));

		// and read back.
		assert_eq!(TestMap::read(&rt, 10), Ok(Something(10)));

		// and mutate in place.
		let update = |old: &mut Something| {
			if old.0 == 0 {
				*old = Something(11)
			} else {
				*old = Something(old.0 + 1)
			}
		};
		assert_eq!(TestMap::mutate(&rt, 11, update), Ok(()));
		assert_eq!(TestMap::read(&rt, 11), Ok(Something(11)));

		assert_eq!(TestMap::mutate(&rt, 11, update), Ok(()));
		assert_eq!(TestMap::read(&rt, 11), Ok(Something(12)));

		// finally clear.
		assert_eq!(TestMap::clear(&rt, 11), Ok(()));
		assert_eq!(TestMap::read(&rt, 11), Ok(Something(0)));
	}

	#[test]
	fn value_works() {
		let state = RuntimeState::new().as_arc();
		let rt = WorkerRuntime::new(state, 0);

		// reads default
		assert_eq!(TestValue::read(&rt), Ok(vec![]));

		// can write
		assert_eq!(TestValue::write(&rt, vec![1, 2, 3]), Ok(()));

		// and read back.
		assert_eq!(TestValue::read(&rt), Ok(vec![1, 2, 3]));

		// and mutate in place.
		let update = |old: &mut Vec<u32>| {
			if old.len() == 0 {
				*old = vec![1, 3]
			} else {
				old.push(999)
			}
		};

		assert!(TestValue::clear(&rt).is_ok());
		assert_eq!(TestValue::mutate(&rt, update), Ok(()));
		assert_eq!(TestValue::read(&rt), Ok(vec![1, 3]));

		assert_eq!(TestValue::mutate(&rt, update), Ok(()));
		assert_eq!(TestValue::read(&rt), Ok(vec![1, 3, 999]));
	}
}
