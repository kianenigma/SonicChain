#[macro_export]
macro_rules! decl_outer_call {
	(
		$vis:vis enum $outer_call_name:ident {
			$(
				$module_name:ident($inner_call_path:path),
			)*
		}
	) => {
		#[derive(Debug, Clone, Eq, PartialEq, Encode, Decode)]
		$vis enum $outer_call_name {
			$(
				$module_name($inner_call_path)
			),*
		}

		impl<R: $crate::ModuleRuntime> $crate::Dispatchable<R> for $outer_call_name {
			fn dispatch<T: $crate::DispatchPermission>(self, runtime: &R, origin: $crate::AccountId) -> $crate::DispatchResult {
				match self {
					$(
						$outer_call_name::$module_name(inner_call) => {
							<$inner_call_path as $crate::Dispatchable<R>>::dispatch::<T>(inner_call, runtime, origin)
						}
					)*
				}
			}

			fn validate(&self, runtime: &R, origin: $crate::AccountId) -> $crate::ValidationResult {
				match self {
					$(
						$outer_call_name::$module_name(inner_call) => {
							<$inner_call_path as $crate::Dispatchable<R>>::validate(inner_call, runtime, origin)
						}
					)*
				}
			}
		}
	};
}

#[macro_export]
macro_rules! decl_tx {
	// fill default #[access]
	(
		$(
			fn $name:ident(
				$runtime:ident,
				$origin:ident
				$(, $arg_name:ident : $arg_type:ty)* $(,)?
			) { $( $impl:tt )* }
		)*
	) => {
		$crate::decl_tx!(
			$(
				#[access = (|_| { Default::default() })]
				fn $name(
					$runtime,
					$origin
					$(, $arg_name: $arg_type)*
				) { $( $impl )* }
			)*
		);
	};

	// entry arm
	(
		$(
			#[access = $access:tt]
			fn $name:ident(
				$runtime:ident,
				$origin:ident
				$(, $arg_name:ident : $arg_type:ty)* $(,)?
			) { $( $impl:tt )* }
		)*
	) => {
		// expand fn
		$crate::decl_tx!(
			@DECL_FUNCTION
			$(
				fn $name(
					$runtime,
					$origin
					$(, $arg_name : $arg_type)*
				) {  $( $impl )* }
			)*
		);

		// expand call enum
		$crate::decl_tx!(
			@DECL_CALL_ENUM
			$(
				fn $name(
					$(, $arg_name : $arg_type)*
				)
			)*
		);

		// implement dispatchable.
		$crate::decl_tx!(
			@IMPL_DISPATCHABLE
			$(
				#[access = $access]
				fn $name(
					$(, $arg_name : $arg_type)*
				)
			)*
		);
	};

	// arm1: decl_function.
	(
		@DECL_FUNCTION
		$(
			fn $name:ident(
				$runtime:ident,
				$origin:ident
				$(, $arg_name:ident : $arg_type:ty)* $(,)?
			) { $( $impl:tt )* }
		)*
	) => {
		$(
			fn $name<
				R: $crate::ModuleRuntime
			>(
				$runtime: &R,
				$origin: $crate::AccountId
				$(, $arg_name: $arg_type)*
			) -> $crate::DispatchResult {
				$( $impl )*
			}
		)*
	};

	// arm2: decl call enum.
	(
		@DECL_CALL_ENUM
		$(
			fn $name:ident(
				$(, $arg_name:ident : $arg_type:ty)* $(,)?
			)
		)*
	) => {
		$crate::paste! {
			/// The call of the this module.
			#[derive(Debug, Clone, Eq, PartialEq, Encode, Decode)]
			pub enum Call {
				$( [<$name:camel>]( $($arg_type),*) ),*
			}
		}
	};

	// arm3: impl dispatchable trait.
	(
		@IMPL_DISPATCHABLE
		$(
			#[access = $access:tt]
			fn $name:ident(
				$(, $arg_name:ident : $arg_type:ty)* $(,)?
			)
		)*
	) => {
		$crate::paste! {
			impl<R: $crate::ModuleRuntime> $crate::Dispatchable<R> for Call {
				fn dispatch<T>(self, runtime: &R, origin: $crate::AccountId) -> $crate::DispatchResult {
					match self {
						$(
							Self::[<$name:camel>]( $($arg_name),* ) => $name(runtime, origin, $($arg_name),*)
						),*
					}
				}

				// TODO: validation result seem to NOT need to be a result after all.
				#[allow(unused)]
				fn validate(&self, _: &R, origin: AccountId) -> $crate::ValidationResult {
					match self {
						$(
							Self::[<$name:camel>]( $($arg_name),* ) => Ok($access(origin)),
						)*
					}
				}
			}
		}
	}
}

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
mod tests_storage_macros {
	use crate::{ConcurrentRuntime, RuntimeState};
	use parity_scale_codec::{Decode, Encode};
	use primitives::{testing, AccountId};

	const MODULE: &'static str = "test";

	#[derive(Encode, Decode, Debug, Clone, Default, Eq, PartialEq)]
	pub struct Something(pub u32);

	decl_storage_map!(TestMap, "map", u8, Something);
	decl_storage_map!(AccMap, "accMap", AccountId, AccountId);
	decl_storage_value!(TestValue, "value", Vec<u32>);

	#[test]
	fn map_account_id_default_works() {
		let state = RuntimeState::new().as_arc();
		let rt = ConcurrentRuntime::new(state, 0);

		assert_eq!(
			AccMap::read(&rt, testing::alice().public()).unwrap(),
			testing::default().public()
		);
	}

	#[test]
	fn map_works() {
		let state = RuntimeState::new().as_arc();
		let rt = ConcurrentRuntime::new(state, 0);

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
		let rt = ConcurrentRuntime::new(state, 0);

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
