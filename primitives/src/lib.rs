use ed25519_dalek as edc;
use parity_scale_codec as codec;

/// The key of storage items.
///
/// We use string for simplicity; The string itself can be the hex encoded value of other hashes.
pub type Key = Vec<u8>;
/// Values inserted into storage.
pub type Value = Vec<u8>;
/// identifier of a thread.
pub type ThreadId = u64;
/// The account identifier.
pub type AccountId = Public;
/// The balance type.
pub type Balance = u128;

/// A public key.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct Public(edc::PublicKey);

impl codec::Encode for Public {
	fn encode(&self) -> Vec<u8> {
		let mut r = Vec::with_capacity(edc::PUBLIC_KEY_LENGTH);
		r.extend(self.as_ref());
		r
	}
}

impl codec::Decode for Public {
	fn decode<I: codec::Input>(value: &mut I) -> Result<Self, codec::Error> {
		let mut bytes = [0u8; edc::PUBLIC_KEY_LENGTH];
		value
			.read(&mut bytes)
			.expect("Encoded public mut be exactly `PUBLIC_KEY_LENGTH`");
		Self::from_bytes(&bytes)
			.map_err(|_| codec::Error::from("Failed to build public from bytes"))
	}
}

impl Public {
	pub fn from_bytes(bytes: &[u8]) -> Result<Self, edc::SignatureError> {
		edc::PublicKey::from_bytes(bytes).map(|inner| Self(inner))
	}

	/// Generates a random public key, throwing away the private part.
	///
	/// Should only be used for testing.
	pub fn random() -> Self {
		let mut csprng = rand::rngs::OsRng {};
		let keypair = edc::Keypair::generate(&mut csprng);
		Self(keypair.public)
	}
}

impl From<edc::PublicKey> for Public {
	fn from(p: edc::PublicKey) -> Self {
		Self(p)
	}
}

impl AsRef<[u8]> for Public {
	fn as_ref(&self) -> &[u8] {
		self.0.as_ref()
	}
}

pub struct Pair(edc::Keypair);

impl Pair {
	pub fn public(&self) -> Public {
		Public::from(self.0.public)
	}

	pub fn private(self) -> Private {
		Private::from(self.0.secret)
	}

	pub fn sign(&self, message: &[u8]) -> Signature {
		Signature(self.0.sign(message))
	}

	pub fn verify(&self, message: &[u8], signature: &Signature) -> bool {
		self.0.verify(message, &signature.0).is_ok()
	}
}

impl std::fmt::Debug for Pair {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Pair")
			.field("Private", &self.0.secret.as_ref())
			.field("Public", &self.0.public.as_ref())
			.finish()
	}
}

#[derive(Debug)]
pub struct Private(edc::SecretKey);

impl From<edc::SecretKey> for Private {
	fn from(s: edc::SecretKey) -> Self {
		Self(s)
	}
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct Signature(edc::Signature);

impl From<edc::Signature> for Signature {
	fn from(sig: edc::Signature) -> Self {
		Self(sig)
	}
}

pub mod testing {
	use super::*;

	pub fn alice() -> Pair {
		let bytes = vec![
			244, 5, 58, 42, 41, 213, 241, 132, 127, 57, 115, 17, 45, 124, 96, 105, 153, 122, 117,
			16, 191, 116, 208, 222, 115, 211, 181, 4, 135, 120, 232, 225, 164, 60, 189, 222, 17,
			247, 124, 143, 160, 82, 185, 246, 157, 20, 106, 182, 97, 30, 181, 158, 71, 93, 35, 179,
			171, 251, 111, 198, 206, 180, 249, 81,
		];
		Pair(edc::Keypair::from_bytes(bytes.as_ref()).unwrap())
	}

	pub fn bob() -> Pair {
		let bytes = vec![
			169, 102, 220, 55, 249, 160, 36, 85, 87, 108, 33, 38, 8, 76, 149, 152, 246, 193, 111,
			201, 121, 131, 237, 99, 11, 113, 80, 143, 25, 172, 67, 196, 75, 236, 184, 74, 110, 177,
			248, 89, 181, 88, 105, 72, 121, 68, 121, 212, 49, 155, 114, 162, 138, 220, 64, 115, 12,
			203, 13, 204, 232, 80, 197, 128,
		];
		Pair(edc::Keypair::from_bytes(bytes.as_ref()).unwrap())
	}

	pub fn dave() -> Pair {
		let bytes = vec![
			67, 204, 236, 61, 177, 124, 97, 247, 255, 113, 195, 14, 108, 91, 93, 12, 22, 12, 41,
			58, 87, 216, 50, 66, 119, 85, 159, 68, 236, 83, 214, 122, 52, 250, 190, 45, 30, 123,
			179, 245, 198, 198, 171, 122, 42, 39, 219, 252, 155, 118, 172, 3, 42, 140, 242, 58,
			182, 214, 35, 33, 6, 134, 37, 202,
		];
		Pair(edc::Keypair::from_bytes(bytes.as_ref()).unwrap())
	}

	pub fn eve() -> Pair {
		let bytes = vec![
			185, 198, 116, 114, 180, 24, 166, 247, 113, 53, 68, 236, 101, 157, 192, 36, 178, 245,
			184, 163, 229, 2, 237, 118, 239, 109, 177, 222, 173, 126, 134, 152, 204, 62, 140, 165,
			109, 43, 220, 109, 69, 91, 85, 247, 59, 82, 203, 199, 103, 205, 90, 69, 28, 20, 51,
			193, 226, 144, 69, 204, 33, 88, 36, 191,
		];
		Pair(edc::Keypair::from_bytes(bytes.as_ref()).unwrap())
	}

	pub fn random() -> Pair {
		let mut csprng = rand::rngs::OsRng {};
		let keypair = edc::Keypair::generate(&mut csprng);
		Pair(keypair)
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
