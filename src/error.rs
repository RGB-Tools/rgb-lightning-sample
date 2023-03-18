/// The error variants returned by functions
#[derive(Debug, thiserror::Error)]
pub enum Error {
	#[error("IO error: {0}")]
	IO(#[from] std::io::Error),

	#[error("Proxy error: {0}")]
	Proxy(#[from] reqwest::Error),

	#[error("ERROR: no uncolored UTXOs are available (hint: call createutxos)")]
	NoAvailableUtxos,

	#[error("ERROR: unknown RGB contract ID")]
	UnknownContractId,
}
