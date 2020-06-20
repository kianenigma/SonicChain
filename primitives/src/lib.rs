use ed25519_dalek as edc;

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

impl AsRef<[u8]> for Public {
	fn as_ref(&self) -> &[u8] {
		self.0.as_ref()
	}
}
