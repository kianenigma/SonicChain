use std::fmt::{Debug, Formatter, Result as FmtResult};

// re-export crypto stuff.
mod crypto;
pub use crypto::*;

/// The key of storage items.
pub type Key = StateKey;
/// Values inserted into storage.
pub type Value = StateValue;
/// identifier of a thread.
pub type ThreadId = u64;
/// The account identifier.
pub type AccountId = Public;
/// The balance type.
pub type Balance = u128;
/// Identifier of a transaction.
pub type TransactionId = u32;

/// A struct for better printing of slice types.
pub struct Slice<'a>(&'a [u8]);

impl<'a> Slice<'a> {
	pub fn new<T>(data: &'a T) -> Self
	where
		T: ?Sized + AsRef<[u8]> + 'a,
	{
		Slice(data.as_ref())
	}
}

impl<'a> Debug for Slice<'a> {
	fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
		if self.0.len() > 6 {
			write!(
				f,
				"[{}, {}, {} .. {}, {}, {}]",
				self.0[0],
				self.0[1],
				self.0[2],
				self.0[self.0.len() - 3],
				self.0[self.0.len() - 2],
				self.0[self.0.len() - 2]
			)
		} else {
			write!(f, "{:?}", self.0)
		}
	}
}

/// Extension trait for byte display.
pub trait BytesDisplay {
	fn bytes_display(&self) -> Slice<'_>;
}

impl<T: ?Sized + AsRef<[u8]>> BytesDisplay for T {
	fn bytes_display(&self) -> Slice<'_> {
		Slice::new(self)
	}
}

/// Struct for better hex printing of slice types.
pub struct HexSlice<'a>(&'a [u8]);

impl<'a> HexSlice<'a> {
	pub fn new<T>(data: &'a T) -> HexSlice<'a>
	where
		T: ?Sized + AsRef<[u8]> + 'a,
	{
		HexSlice(data.as_ref())
	}
}

// You can choose to implement multiple traits, like Lower and UpperHex
impl Debug for HexSlice<'_> {
	fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
		write!(f, "0x")?;
		for byte in self.0 {
			write!(f, "{:x}", byte)?;
		}
		Ok(())
	}
}

/// Extension trait for hex display.
pub trait HexDisplayExt {
	fn hex_display(&self) -> HexSlice<'_>;
}

impl<T: ?Sized + AsRef<[u8]>> HexDisplayExt for T {
	fn hex_display(&self) -> HexSlice<'_> {
		HexSlice::new(self)
	}
}

/// A state key.
#[derive(Clone, Eq, PartialEq, Hash, Default, Ord, PartialOrd)]
pub struct StateKey(pub Vec<u8>);

impl From<Vec<u8>> for StateKey {
	fn from(t: Vec<u8>) -> Self {
		Self(t)
	}
}

impl AsRef<[u8]> for StateKey {
	fn as_ref(&self) -> &[u8] {
		self.0.as_ref()
	}
}

impl Debug for StateKey {
	fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
		write!(f, "Key [{:?}]", &self.0.hex_display())
	}
}

/// A state value.
#[derive(Clone, Eq, PartialEq, Hash, Default)]
pub struct StateValue(pub Vec<u8>);

impl From<Vec<u8>> for StateValue {
	fn from(t: Vec<u8>) -> Self {
		Self(t)
	}
}

impl AsRef<[u8]> for StateValue {
	fn as_ref(&self) -> &[u8] {
		self.0.as_ref()
	}
}

impl Debug for StateValue {
	fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
		write!(f, "Value [{:?}]", &self.0.hex_display())
	}
}

#[cfg(test)]
mod primitive_tests {
	use super::*;
	use parity_scale_codec::{Decode, Encode};

	#[test]
	fn public_is_codec() {
		let public = testing::alice().public();
		let encoded = public.encode();
		assert_eq!(encoded, public.as_ref());
		assert_eq!(Public::decode(&mut &*encoded).unwrap(), public);
	}
}
