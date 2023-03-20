use crate::error::Error;
use amplify::s;
use reqwest::header::CONTENT_TYPE;
use reqwest::{multipart, Body, Client};
use serde::{Deserialize, Serialize};
use tokio::fs::File;
use tokio_util::codec::{BytesCodec, FramedRead};

use std::path::PathBuf;

const JSON: &str = "application/json";

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct JsonRpcError {
	pub(crate) code: i64,
	message: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct JsonRpcRequest<P> {
	method: String,
	jsonrpc: String,
	id: Option<String>,
	params: Option<P>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct JsonRpcResponse<R> {
	id: Option<String>,
	pub(crate) result: Option<R>,
	pub(crate) error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BlindedUtxoParam {
	blinded_utxo: String,
}

pub trait Proxy {
	fn post_consignment(
		self, url: &str, blindedutxo: String, consignment_path: PathBuf,
	) -> Result<JsonRpcResponse<bool>, Error>;
	fn get_consignment(
		self, url: &str, blindedutxo: String,
	) -> Result<JsonRpcResponse<String>, reqwest::Error>;
}

impl Proxy for Client {
	fn post_consignment(
		self, url: &str, consignment_id: String, consignment_path: PathBuf,
	) -> Result<JsonRpcResponse<bool>, Error> {
		futures::executor::block_on(async {
			let file = File::open(consignment_path.clone()).await?;
			let stream = FramedRead::new(file, BytesCodec::new());
			let file_name = consignment_path
				.clone()
				.file_name()
				.map(|filename| filename.to_string_lossy().into_owned())
				.expect("valid file name");
			let consignment_file =
				multipart::Part::stream(Body::wrap_stream(stream)).file_name(file_name);

			let params = serde_json::to_string(&BlindedUtxoParam { blinded_utxo: consignment_id })
				.expect("valid param");
			let form = multipart::Form::new()
				.text("method", "consignment.post")
				.text("jsonrpc", "2.0")
				.text("id", "1")
				.text("params", params)
				.part("file", consignment_file);
			Ok(self.post(url).multipart(form).send().await?.json::<JsonRpcResponse<bool>>().await?)
		})
	}

	fn get_consignment(
		self, url: &str, consignment_id: String,
	) -> Result<JsonRpcResponse<String>, reqwest::Error> {
		futures::executor::block_on(async {
			let body = JsonRpcRequest {
				method: s!("consignment.get"),
				jsonrpc: s!("2.0"),
				id: None,
				params: Some(BlindedUtxoParam { blinded_utxo: consignment_id }),
			};
			self.post(url)
				.header(CONTENT_TYPE, JSON)
				.json(&body)
				.send()
				.await?
				.json::<JsonRpcResponse<String>>()
				.await
		})
	}
}
