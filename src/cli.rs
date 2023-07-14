use crate::bdk_utils::sync_wallet;
use crate::bitcoind_client::BitcoindClient;
use crate::broadcast_tx;
use crate::disk;
use crate::error::Error;
use crate::hex_utils;
use crate::proxy::{get_consignment, post_consignment};
use crate::rgb_utils::get_asset_owned_values;
use crate::rgb_utils::get_rgb_total_amount;
use crate::rgb_utils::RgbUtilities;
use crate::swap::{get_current_timestamp, SwapString, SwapType};
use crate::{
	ChannelManager, HTLCStatus, MillisatAmount, NetworkGraph, OnionMessenger, PaymentInfo,
	PaymentInfoStorage, PeerManager, Router,
};
use crate::{FEE_RATE, UTXO_SIZE_SAT};

use amplify::none;
use bdk::bitcoin::hashes::Hash;
use bdk::bitcoin::OutPoint;
use bdk::bitcoin::Txid;
use bdk::database::SqliteDatabase;
use bdk::{FeeRate, SignOptions, Wallet};
use bitcoin::hashes::sha256::Hash as Sha256;
use bitcoin::network::constants::Network;
use bitcoin::secp256k1::PublicKey;
use bp::seals::txout::CloseMethod;
use bp::seals::txout::ExplicitSeal;
use lightning::chain::keysinterface::{EntropySource, KeysManager};
use lightning::ln::channelmanager::{PaymentId, RecipientOnionFields, Retry};
use lightning::ln::msgs::NetAddress;
use lightning::ln::PaymentSecret;
use lightning::ln::{PaymentHash, PaymentPreimage};
use lightning::onion_message::{CustomOnionMessageContents, Destination, OnionMessageContents};
use lightning::rgb_utils::drop_rgb_runtime;
use lightning::rgb_utils::get_rgb_runtime;
use lightning::rgb_utils::write_rgb_payment_info_file;
use lightning::rgb_utils::{
	get_rgb_channel_info, write_rgb_channel_info, RgbInfo, RgbUtxo, RgbUtxos,
};
use lightning::routing::gossip::NodeId;
use lightning::routing::gossip::RoutingFees;
use lightning::routing::router::RouteHint;
use lightning::routing::router::RouteHintHop;
use lightning::routing::router::{
	Hints, Path as LnPath, Route, Router as RouterTrait, DEFAULT_MAX_TOTAL_CLTV_EXPIRY_DELTA,
};
use lightning::routing::router::{PaymentParameters, RouteParameters};
use lightning::util::config::{ChannelHandshakeConfig, ChannelHandshakeLimits, UserConfig};
use lightning::util::ser::{Writeable, Writer};
use lightning::util::IS_SWAP_SCID;
use lightning_invoice::payment::pay_invoice;
use lightning_invoice::{utils, Currency, Invoice};
use reqwest::Client as RestClient;
use rgb_core::validation::Validity;
use rgbstd::containers::Bindle;
use rgbstd::containers::BuilderSeal;
use rgbstd::containers::Transfer as RgbTransfer;
use rgbstd::contract::{ContractId, GraphSeal, SecretSeal};
use rgbstd::interface::TypedState;
use rgbstd::persistence::Inventory;
use rgbstd::Txid as RgbTxid;
use rgbwallet::RgbTransport;
use seals::txout::blind::BlindSeal;
use seals::txout::TxPtr;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};
use std::env;
use std::fs;
use std::io;
use std::io::Write;
use std::net::{SocketAddr, ToSocketAddrs};
use std::ops::Deref;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use strict_encoding::{FieldName, TypeName};

const MIN_CREATE_UTXOS_SATS: u64 = 10000;
const UTXO_NUM: u8 = 4;

const OPENCHANNEL_MIN_SAT: u64 = 5506;
const OPENCHANNEL_MAX_SAT: u64 = 16777215;

const DUST_LIMIT_MSAT: u64 = 546000;

pub(crate) const HTLC_MIN_MSAT: u64 = 3000000;

const INVOICE_MIN_MSAT: u64 = HTLC_MIN_MSAT;

#[derive(Serialize, Deserialize)]
struct BlindedInfo {
	contract_id: Option<ContractId>,
	seal: BlindSeal<TxPtr>,
	consumed: bool,
}

pub(crate) struct LdkUserInfo {
	pub(crate) bitcoind_rpc_username: String,
	pub(crate) bitcoind_rpc_password: String,
	pub(crate) bitcoind_rpc_port: u16,
	pub(crate) bitcoind_rpc_host: String,
	pub(crate) ldk_storage_dir_path: String,
	pub(crate) ldk_peer_listening_port: u16,
	pub(crate) ldk_announced_listen_addr: Vec<NetAddress>,
	pub(crate) ldk_announced_node_name: [u8; 32],
	pub(crate) network: Network,
}

struct UserOnionMessageContents {
	tlv_type: u64,
	data: Vec<u8>,
}

impl CustomOnionMessageContents for UserOnionMessageContents {
	fn tlv_type(&self) -> u64 {
		self.tlv_type
	}
}

impl Writeable for UserOnionMessageContents {
	fn write<W: Writer>(&self, w: &mut W) -> Result<(), std::io::Error> {
		w.write_all(&self.data)
	}
}

pub(crate) async fn poll_for_user_input(
	peer_manager: Arc<PeerManager>, channel_manager: Arc<ChannelManager>,
	keys_manager: Arc<KeysManager>, network_graph: Arc<NetworkGraph>,
	onion_messenger: Arc<OnionMessenger>, router: Arc<Router>,
	inbound_payments: PaymentInfoStorage, outbound_payments: PaymentInfoStorage,
	ldk_data_dir: String, network: Network, logger: Arc<disk::FilesystemLogger>,
	bitcoind_client: Arc<BitcoindClient>, proxy_client: Arc<RestClient>, proxy_url: &str,
	proxy_endpoint: &str, wallet_arc: Arc<Mutex<Wallet<SqliteDatabase>>>, electrum_url: String,
	whitelisted_trades: Arc<Mutex<HashMap<PaymentHash, (ContractId, SwapType)>>>,
	maker_trades: Arc<Mutex<HashMap<PaymentHash, (ContractId, SwapType)>>>,
) {
	println!(
		"LDK startup successful. Enter \"help\" to view available commands. Press Ctrl-D to quit."
	);
	println!("LDK logs are available at <your-supplied-ldk-data-dir-path>/.ldk/logs");
	println!("Local Node ID is {}", channel_manager.get_our_node_id());
	loop {
		print!("> ");
		io::stdout().flush().unwrap(); // Without flushing, the `>` doesn't print
		let mut line = String::new();
		if let Err(e) = io::stdin().read_line(&mut line) {
			break println!("ERROR: {}", e);
		}

		if line.len() == 0 {
			// We hit EOF / Ctrl-D
			break;
		}

		let mut words = line.split_whitespace();
		if let Some(word) = words.next() {
			match word {
				"help" => help(),
				"mine" => {
					if network != Network::Regtest {
						println!("ERROR: mine command is available only on regtest");
						continue;
					}
					let num_blocks = words.next();

					if num_blocks.is_none() {
						println!("ERROR: mine has 1 required argument: `mine num_blocks`");
						continue;
					}

					let num_blocks: Result<u16, _> = num_blocks.unwrap().parse();
					if num_blocks.is_err() {
						println!("ERROR: num_blocks must be a number");
						continue;
					}

					mine(&bitcoind_client, num_blocks.unwrap()).await;
				}
				"listunspent" => {
					let wallet = wallet_arc.lock().unwrap();
					let unspents = wallet.list_unspent().expect("unspents");
					println!("Unspents:");
					for unspent in unspents {
						println!(" - {unspent:?}");
					}
				}
				"getaddress" => {
					let wallet = wallet_arc.lock().unwrap();
					let address = wallet
						.get_address(bdk::wallet::AddressIndex::New)
						.expect("valid address")
						.address;
					println!("Address: {address}");
				}
				"createutxos" => {
					let wallet = wallet_arc.lock().unwrap();
					sync_wallet(&wallet, electrum_url.clone());

					let rgb_utxos_path = format!("{}/rgb_utxos", ldk_data_dir.clone());
					let serialized_utxos =
						fs::read_to_string(&rgb_utxos_path).expect("able to read rgb utxos file");
					let mut rgb_utxos: RgbUtxos =
						serde_json::from_str(&serialized_utxos).expect("valid rgb utxos");
					let unspendable_utxos: Vec<OutPoint> =
						rgb_utxos.utxos.iter().map(|u| u.outpoint).collect();

					let unspendable_amt: u64 = wallet
						.list_unspent()
						.expect("unspents")
						.iter()
						.filter(|u| unspendable_utxos.contains(&u.outpoint))
						.map(|u| u.txout.value)
						.sum();
					let available =
						wallet.get_balance().expect("wallet balance").get_total() - unspendable_amt;
					if available < MIN_CREATE_UTXOS_SATS {
						println!(
							"ERROR: not enough funds, call getaddress and send {} satoshis",
							MIN_CREATE_UTXOS_SATS - available
						);
						continue;
					}

					let mut tx_builder = wallet.build_tx();
					tx_builder
						.unspendable(unspendable_utxos)
						.fee_rate(FeeRate::from_sat_per_vb(FEE_RATE))
						.ordering(bdk::wallet::tx_builder::TxOrdering::Untouched);
					for _i in 0..UTXO_NUM {
						tx_builder.add_recipient(
							wallet
								.get_address(bdk::wallet::AddressIndex::New)
								.expect("address")
								.script_pubkey(),
							UTXO_SIZE_SAT,
						);
					}
					let (mut psbt, _details) =
						tx_builder.finish().expect("successful psbt creation");

					wallet.sign(&mut psbt, SignOptions::default()).expect("successful sign");

					let tx = psbt.extract_tx();
					broadcast_tx(&tx, electrum_url.clone());

					for i in 0..UTXO_NUM {
						rgb_utxos.utxos.push(RgbUtxo {
							outpoint: OutPoint { txid: tx.txid(), vout: i as u32 },
							colored: false,
						});
					}
					let serialized_utxos =
						serde_json::to_string(&rgb_utxos).expect("valid rgb utxos");
					fs::write(rgb_utxos_path, serialized_utxos)
						.expect("able to write rgb utxos file");

					sync_wallet(&wallet, electrum_url.clone());
					println!("UTXO creation complete");
				}
				"issueasset" => {
					let amount = words.next();
					let ticker = words.next();
					let name = words.next();
					let precision = words.next();

					if amount.is_none() || ticker.is_none() || name.is_none() || precision.is_none()
					{
						println!("ERROR: issueasset has 4 required arguments: `issueasset <amount> <ticker> <name> <precision>`");
						continue;
					}

					let amount: Result<u64, _> = amount.unwrap().parse();
					if amount.is_err() {
						println!("ERROR: amount must be a number");
						continue;
					}

					let ticker = ticker.unwrap();
					if !ticker.is_ascii() {
						println!("ERROR: ticker must be an ASCII string");
						continue;
					}

					let name = name.unwrap();
					if !name.is_ascii() {
						println!("ERROR: name must be an ASCII string");
						continue;
					}

					let precision: Result<u8, _> = precision.unwrap().parse();
					if precision.is_err() {
						println!("ERROR: precision must be a number");
						continue;
					}

					match check_uncolored_utxos(&ldk_data_dir).await {
						Ok(_) => {}
						Err(e) => {
							println!("{e}");
							continue;
						}
					}
					let outpoint = get_utxo(&ldk_data_dir).await.outpoint;
					let rgb_utxos_path = format!("{}/rgb_utxos", ldk_data_dir.clone());
					let serialized_utxos =
						fs::read_to_string(&rgb_utxos_path).expect("able to read rgb utxos file");
					let mut rgb_utxos: RgbUtxos =
						serde_json::from_str(&serialized_utxos).expect("valid rgb utxos");
					rgb_utxos
						.utxos
						.iter_mut()
						.find(|u| u.outpoint == outpoint)
						.expect("UTXO found")
						.colored = true;
					let serialized_utxos =
						serde_json::to_string(&rgb_utxos).expect("valid rgb utxos");
					fs::write(rgb_utxos_path, serialized_utxos)
						.expect("able to write rgb utxos file");

					let mut runtime = get_rgb_runtime(&PathBuf::from(ldk_data_dir.clone()));
					let contract_id = runtime.issue_contract(
						amount.unwrap(),
						outpoint,
						ticker.to_string(),
						name.to_string(),
						precision.unwrap(),
					);
					drop(runtime);
					drop_rgb_runtime(&PathBuf::from(ldk_data_dir.clone()));
					println!("Asset ID: {contract_id}");
				}
				"assetbalance" => {
					let assetbalance_cmd = "`assetbalance <contract_id>`";
					let contract_id = words.next();

					if contract_id.is_none() {
						println!(
							"ERROR: assetbalance has 1 required argument: `{assetbalance_cmd}`"
						);
						continue;
					}

					let contract_id = ContractId::from_str(contract_id.unwrap());
					if contract_id.is_err() {
						println!("ERROR: contract_id must be a valid RGB asset ID");
						continue;
					}

					let runtime = get_rgb_runtime(&PathBuf::from(ldk_data_dir.clone()));
					let total_rgb_amount = match get_rgb_total_amount(
						contract_id.unwrap(),
						&runtime,
						wallet_arc.clone(),
						electrum_url.clone(),
					) {
						Ok(a) => a,
						Err(e) => {
							println!("{e}");
							continue;
						}
					};

					drop(runtime);
					drop_rgb_runtime(&PathBuf::from(ldk_data_dir.clone()));

					println!("Asset balance: {total_rgb_amount}");
				}
				"sendasset" => {
					let sendasset_cmd = "`sendasset <contract_id> <amt_rgb> <blinded_utxo>`";
					let contract_id = words.next();
					let amt_rgb_str = words.next();
					let blinded_utxo = words.next();

					if contract_id.is_none() || amt_rgb_str.is_none() || blinded_utxo.is_none() {
						println!("ERROR: sendasset has 3 required arguments: `{sendasset_cmd}`");
						continue;
					}

					let contract_id = ContractId::from_str(contract_id.unwrap());
					if contract_id.is_err() {
						println!("ERROR: contract_id must be a valid RGB asset ID");
						continue;
					}
					let contract_id = contract_id.unwrap();

					let amt_rgb: u64 = match amt_rgb_str.unwrap().parse() {
						Ok(amt) => amt,
						Err(e) => {
							println!("ERROR: couldn't parse amt_rgb: {e}");
							continue;
						}
					};

					let mut runtime = get_rgb_runtime(&PathBuf::from(ldk_data_dir.clone()));

					let total_rgb_amount = match get_rgb_total_amount(
						contract_id,
						&runtime,
						wallet_arc.clone(),
						electrum_url.clone(),
					) {
						Ok(a) => a,
						Err(e) => {
							println!("{e}");
							continue;
						}
					};

					if amt_rgb > total_rgb_amount {
						println!("ERROR: do not have enough RGB assets");
						continue;
					}

					let blinded_utxo = blinded_utxo.unwrap();
					let concealed_seal = SecretSeal::from_str(blinded_utxo);
					if concealed_seal.is_err() {
						println!("ERROR: blinded_utxo must be a valid RGB blinded UTXO");
						continue;
					}
					let concealed_seal = concealed_seal.unwrap();

					let asset_owned_values = get_asset_owned_values(
						contract_id,
						&runtime,
						wallet_arc.clone(),
						electrum_url.clone(),
					)
					.expect("known contract");

					let mut asset_transition_builder = runtime
						.transition_builder(
							contract_id,
							TypeName::try_from("RGB20").unwrap(),
							None::<&str>,
						)
						.expect("ok");
					let assignment_id = asset_transition_builder
						.assignments_type(&FieldName::from("beneficiary"))
						.expect("valid assignment");
					let mut beneficiaries = vec![];

					let mut rgb_inputs = vec![];
					let mut input_amount: u64 = 0;
					for (_opout, (outpoint, amount)) in asset_owned_values {
						if input_amount >= amt_rgb {
							break;
						}
						rgb_inputs.push(OutPoint {
							txid: Txid::from_str(&outpoint.txid.to_string()).unwrap(),
							vout: outpoint.vout.into_u32(),
						});
						input_amount += amount;
					}

					let rgb_change_amount = input_amount - amt_rgb;
					if rgb_change_amount > 0 {
						match check_uncolored_utxos(&ldk_data_dir).await {
							Ok(_) => {}
							Err(e) => {
								println!("{e}");
								continue;
							}
						}
						let rgb_change_outpoint = get_utxo(&ldk_data_dir).await.outpoint;
						let rgb_utxos_path = format!("{}/rgb_utxos", ldk_data_dir.clone());
						let serialized_utxos = fs::read_to_string(&rgb_utxos_path)
							.expect("able to read rgb utxos file");
						let mut rgb_utxos: RgbUtxos =
							serde_json::from_str(&serialized_utxos).expect("valid rgb utxos");
						rgb_utxos
							.utxos
							.iter_mut()
							.find(|u| u.outpoint == rgb_change_outpoint)
							.expect("UTXO found")
							.colored = true;
						let serialized_utxos =
							serde_json::to_string(&rgb_utxos).expect("valid rgb utxos");
						fs::write(rgb_utxos_path, serialized_utxos)
							.expect("able to write rgb utxos file");
						let seal = ExplicitSeal::with(
							CloseMethod::OpretFirst,
							RgbTxid::from_str(&rgb_change_outpoint.txid.to_string())
								.unwrap()
								.into(),
							rgb_change_outpoint.vout,
						);
						let seal = GraphSeal::from(seal);
						let change = TypedState::Amount(rgb_change_amount);
						asset_transition_builder = asset_transition_builder
							.add_raw_state(assignment_id, seal, change)
							.unwrap();
					}

					let wallet = wallet_arc.lock().unwrap();

					let mut builder = wallet.build_tx();
					let address = wallet
						.get_address(bdk::wallet::AddressIndex::New)
						.expect("valid address")
						.address;
					builder
						.add_utxos(&rgb_inputs)
						.expect("valid utxos")
						.add_data(&[1])
						.fee_rate(FeeRate::from_sat_per_vb(FEE_RATE))
						.manually_selected_only()
						.drain_to(address.script_pubkey());
					let psbt = builder.finish().expect("valid psbt finish").0;

					asset_transition_builder = asset_transition_builder
						.add_raw_state(assignment_id, concealed_seal, TypedState::Amount(amt_rgb))
						.expect("ok");
					beneficiaries.push(BuilderSeal::Concealed(concealed_seal));

					let (mut psbt, consignment) = runtime.send_rgb(
						contract_id,
						psbt,
						asset_transition_builder,
						beneficiaries,
					);

					let consignment_path = format!("{}/consignment", ldk_data_dir.clone());
					consignment.save(&consignment_path).expect("successful save");

					let proxy_ref = (*proxy_client).clone();
					let res = post_consignment(
						proxy_ref,
						proxy_url,
						blinded_utxo.to_string(),
						consignment_path.into(),
					)
					.await;
					if res.is_err() || res.unwrap().result.is_none() {
						println!("ERROR: unable to post consignment");
						continue;
					}

					wallet.sign(&mut psbt, SignOptions::default()).expect("able to sign");
					let tx = psbt.extract_tx();
					broadcast_tx(&tx, electrum_url.clone());

					let transfer =
						consignment.unbindle().validate(runtime.resolver()).unwrap_or_else(|c| c);
					let _status = runtime.accept_transfer(transfer, true).expect("valid transfer");
					drop(runtime);
					drop_rgb_runtime(&PathBuf::from(ldk_data_dir.clone()));

					sync_wallet(&wallet, electrum_url.clone());
					println!("RGB send complete, txid: {}", tx.txid());
				}
				"receiveasset" => {
					match check_uncolored_utxos(&ldk_data_dir).await {
						Ok(_) => {}
						Err(e) => {
							println!("{e}");
							continue;
						}
					}
					let outpoint = get_utxo(&ldk_data_dir).await.outpoint;
					let rgb_utxos_path = format!("{}/rgb_utxos", ldk_data_dir.clone());
					let serialized_utxos =
						fs::read_to_string(&rgb_utxos_path).expect("able to read rgb utxos file");
					let mut rgb_utxos: RgbUtxos =
						serde_json::from_str(&serialized_utxos).expect("valid rgb utxos");
					rgb_utxos
						.utxos
						.iter_mut()
						.find(|u| u.outpoint == outpoint)
						.expect("UTXO found")
						.colored = true;
					let serialized_utxos =
						serde_json::to_string(&rgb_utxos).expect("valid rgb utxos");
					fs::write(rgb_utxos_path, serialized_utxos)
						.expect("able to write rgb utxos file");

					let seal = ExplicitSeal::with(
						CloseMethod::OpretFirst,
						RgbTxid::from_str(&outpoint.txid.to_string()).unwrap().into(),
						outpoint.vout,
					);
					let seal = GraphSeal::from(seal);

					let mut runtime = get_rgb_runtime(&PathBuf::from(ldk_data_dir.clone()));
					runtime.store_seal_secret(seal).expect("successful seal store");
					drop(runtime);
					drop_rgb_runtime(&PathBuf::from(ldk_data_dir.clone()));

					let concealed_seal = seal.to_concealed_seal();
					let blinded_utxo = concealed_seal.to_string();

					let blinded_dir = PathBuf::from_str(&ldk_data_dir.clone())
						.expect("valid data dir")
						.join("blinded_utxos");
					let blinded_path = blinded_dir.join(&blinded_utxo);
					let blinded_info = BlindedInfo { contract_id: None, seal, consumed: false };
					let serialized_info =
						serde_json::to_string(&blinded_info).expect("valid rgb info");
					fs::write(blinded_path, serialized_info).expect("successful file write");

					println!("Blinded UTXO: {blinded_utxo}");
				}
				"refresh" => {
					let blinded_dir = PathBuf::from_str(&ldk_data_dir)
						.expect("valid data dir")
						.join("blinded_utxos");
					let blinded_files = fs::read_dir(blinded_dir).expect("successfult dir read");

					for bf in blinded_files {
						let serialized_info = fs::read_to_string(bf.as_ref().unwrap().path())
							.expect("valid blinded info file");
						let blinded_info: BlindedInfo =
							serde_json::from_str(&serialized_info).expect("valid blinded data");
						if blinded_info.consumed {
							continue;
						}

						let blinded_utxo = blinded_info.seal.to_concealed_seal().to_string();

						let proxy_ref = (*proxy_client).clone();
						let res = get_consignment(proxy_ref, proxy_url, blinded_utxo.clone()).await;
						if res.is_err() || res.as_ref().unwrap().result.is_none() {
							println!("WARNING: unable to get consignment");
							continue;
						}
						let consignment = res.unwrap().result.unwrap();
						let consignment_bytes =
							base64::decode(consignment).expect("valid consignment");
						let consignment_path =
							format!("{}/consignment_{}", ldk_data_dir.clone(), blinded_utxo);
						fs::write(consignment_path.clone(), consignment_bytes)
							.expect("unable to write file");
						let consignment = Bindle::<RgbTransfer>::load(consignment_path)
							.expect("successful consignment load");
						let transfer: RgbTransfer = consignment.clone().unbindle();

						let mut runtime = get_rgb_runtime(&PathBuf::from(ldk_data_dir.clone()));

						let mut minimal_contract = transfer.clone().into_contract();
						minimal_contract.bundles = none!();
						minimal_contract.terminals = none!();
						let minimal_contract_validated =
							match minimal_contract.clone().validate(runtime.resolver()) {
								Ok(consignment) => consignment,
								Err(consignment) => consignment,
							};
						runtime
							.import_contract(minimal_contract_validated)
							.expect("failure importing issued contract");

						let validated_transfer =
							transfer.validate(runtime.resolver()).expect("invalid contract");
						let status = runtime
							.accept_transfer(validated_transfer, true)
							.expect("valid transfer");
						drop(runtime);
						drop_rgb_runtime(&PathBuf::from(ldk_data_dir.clone()));
						let validity = status.validity();
						if !matches!(validity, Validity::Valid) {
							println!("WARNING: error accepting transfer");
							continue;
						}

						let wallet = wallet_arc.lock().unwrap();
						sync_wallet(&wallet, electrum_url.clone());

						fs::remove_file(bf.unwrap().path()).expect("successful file remove");
					}

					println!("Refresh complete");
				}
				"openchannel" | "opencoloredchannel" => {
					let is_colored = word == "opencoloredchannel";
					let peer_pubkey_and_ip_addr = words.next();
					let channel_value_sat = words.next();
					let push_value_msat = words.next();
					let (contract_id, channel_value_rgb) =
						if is_colored { (words.next(), words.next()) } else { (None, None) };
					let announce_channel = match words.next() {
						Some("--public") | Some("--public=true") => true,
						Some("--public=false") => false,
						Some(_) => {
							println!("ERROR: invalid `--public` command format. Valid formats: `--public`, `--public=true` `--public=false`");
							continue;
						}
						None => false,
					};
					if is_colored
						&& (peer_pubkey_and_ip_addr.is_none()
							|| channel_value_sat.is_none()
							|| push_value_msat.is_none() || contract_id.is_none()
							|| channel_value_rgb.is_none())
					{
						println!("ERROR: opencoloredchannel has 5 required arguments: `openchannel pubkey@host:port chan_amt_satoshis push_amt_msatoshis rgb_contract_id chan_amt_rgb` [--public]");
						continue;
					} else if peer_pubkey_and_ip_addr.is_none()
						|| channel_value_sat.is_none()
						|| push_value_msat.is_none()
					{
						println!("ERROR: openchannel has 3 required arguments: `openchannel pubkey@host:port chan_amt_satoshis push_amt_msatoshis` [--public]");
						continue;
					}

					let peer_pubkey_and_ip_addr = peer_pubkey_and_ip_addr.unwrap();
					let (pubkey, peer_addr) =
						match parse_peer_info(peer_pubkey_and_ip_addr.to_string()) {
							Ok(info) => info,
							Err(e) => {
								println!("{:?}", e.into_inner().unwrap());
								continue;
							}
						};

					let chan_amt_sat: Result<u64, _> = channel_value_sat.unwrap().parse();
					if chan_amt_sat.is_err() {
						println!("ERROR: channel amount must be a number");
						continue;
					}
					let chan_amt_sat = chan_amt_sat.unwrap();
					if chan_amt_sat < OPENCHANNEL_MIN_SAT {
						println!(
							"ERROR: channel amount must be equal or higher than {}",
							OPENCHANNEL_MIN_SAT
						);
						continue;
					}
					if chan_amt_sat > OPENCHANNEL_MAX_SAT {
						println!(
							"ERROR: channel amount must be equal or less than {}",
							OPENCHANNEL_MAX_SAT
						);
						continue;
					}

					let push_amt_msat: Result<u64, _> = push_value_msat.unwrap().parse();
					if push_amt_msat.is_err() {
						println!("ERROR: push amount must be a number");
						continue;
					}
					let push_amt_msat = push_amt_msat.unwrap();
					if is_colored && push_amt_msat < DUST_LIMIT_MSAT {
						println!(
							"ERROR: push amount must be equal or higher than the dust limit ({})",
							DUST_LIMIT_MSAT
						);
						continue;
					}

					let rgb_info = if !is_colored {
						None
					} else {
						let contract_id = ContractId::from_str(contract_id.unwrap());
						if contract_id.is_err() {
							println!("ERROR: contract_id must be a valid RGB asset ID");
							continue;
						}
						let contract_id = contract_id.unwrap();

						let chan_amt_rgb: Result<u64, _> = channel_value_rgb.unwrap().parse();
						if chan_amt_rgb.is_err() {
							println!("ERROR: channel RGB amount must be a number");
							continue;
						}
						let chan_amt_rgb = chan_amt_rgb.unwrap();

						let runtime = get_rgb_runtime(&PathBuf::from(ldk_data_dir.clone()));

						let total_rgb_amount = match get_rgb_total_amount(
							contract_id,
							&runtime,
							wallet_arc.clone(),
							electrum_url.clone(),
						) {
							Ok(a) => a,
							Err(e) => {
								println!("{e}");
								continue;
							}
						};
						drop(runtime);
						drop_rgb_runtime(&PathBuf::from(ldk_data_dir.clone()));

						if chan_amt_rgb > total_rgb_amount {
							println!("ERROR: do not have enough RGB assets");
							continue;
						}

						Some(RgbInfo {
							contract_id,
							local_rgb_amount: chan_amt_rgb,
							remote_rgb_amount: 0,
						})
					};

					if connect_peer_if_necessary(pubkey, peer_addr, peer_manager.clone())
						.await
						.is_err()
					{
						continue;
					};

					let open_channel_result = open_channel(
						pubkey,
						chan_amt_sat,
						push_amt_msat,
						announce_channel,
						channel_manager.clone(),
						proxy_endpoint,
						is_colored,
						Arc::clone(&wallet_arc),
						&electrum_url,
					);
					if open_channel_result.is_err() {
						continue;
					}

					let peer_data_path = format!("{}/channel_peer_data", ldk_data_dir);
					let _ = disk::persist_channel_peer(
						Path::new(&peer_data_path),
						peer_pubkey_and_ip_addr,
					);

					if let Some(rgb_info) = rgb_info {
						let temporary_channel_id = open_channel_result.unwrap();
						let channel_rgb_info_path = format!(
							"{}/{}",
							ldk_data_dir.clone(),
							hex::encode(temporary_channel_id)
						);
						write_rgb_channel_info(&PathBuf::from(&channel_rgb_info_path), &rgb_info);
					}
				}
				"sendpayment" => {
					let invoice_str = words.next();
					if invoice_str.is_none() {
						println!("ERROR: sendpayment requires an invoice: `sendpayment <invoice>`");
						continue;
					}

					let invoice = match Invoice::from_str(invoice_str.unwrap()) {
						Ok(inv) => inv,
						Err(e) => {
							println!("ERROR: invalid invoice: {:?}", e);
							continue;
						}
					};

					if let Some(amt_msat) = invoice.amount_milli_satoshis() {
						if amt_msat < INVOICE_MIN_MSAT {
							println!("ERROR: msat amount in invoice cannot be less than {INVOICE_MIN_MSAT}");
							continue;
						}
					} else {
						println!("ERROR: msat amount missing in invoice");
						continue;
					}

					send_payment(
						&*channel_manager,
						&invoice,
						outbound_payments.clone(),
						PathBuf::from(&ldk_data_dir),
					);
				}
				"keysend" | "coloredkeysend" => {
					let is_colored = word == "coloredkeysend";
					let keysend_cmd = "`keysend <dest_pubkey> <amt_msat> <contract_id> <amt_rgb>`";
					let dest_pubkey = match words.next() {
						Some(dest) => match hex_utils::to_compressed_pubkey(dest) {
							Some(pk) => pk,
							None => {
								println!("ERROR: couldn't parse destination pubkey");
								continue;
							}
						},
						None => {
							println!("ERROR: keysend requires a destination pubkey: {keysend_cmd}");
							continue;
						}
					};
					let amt_msat_str =
						match words.next() {
							Some(amt) => amt,
							None => {
								println!("ERROR: keysend requires an amount in millisatoshis: {keysend_cmd}");
								continue;
							}
						};
					let amt_msat: u64 = match amt_msat_str.parse() {
						Ok(amt) => amt,
						Err(e) => {
							println!("ERROR: couldn't parse amount_msat: {}", e);
							continue;
						}
					};
					let rgb_payment = if is_colored {
						if amt_msat < HTLC_MIN_MSAT {
							println!("ERROR: amount_msat cannot be less than {HTLC_MIN_MSAT}");
							continue;
						}
						let contract_id = match words.next() {
							Some(contract_id_str) => match ContractId::from_str(contract_id_str) {
								Ok(cid) => cid,
								Err(_) => {
									println!("ERROR: invalid contract ID: {contract_id_str}");
									continue;
								}
							},
							None => {
								println!("ERROR: keysend requires a contract ID: {keysend_cmd}");
								continue;
							}
						};
						let amt_rgb_str = match words.next() {
							Some(amt) => amt,
							None => {
								println!("ERROR: keysend requires an RGB amount: {keysend_cmd}");
								continue;
							}
						};
						let amt_rgb: u64 = match amt_rgb_str.parse() {
							Ok(amt) => amt,
							Err(e) => {
								println!("ERROR: couldn't parse amt_rgb: {e}");
								continue;
							}
						};
						Some((contract_id, amt_rgb))
					} else {
						None
					};
					keysend(
						&*channel_manager,
						dest_pubkey,
						amt_msat,
						&*keys_manager,
						outbound_payments.clone(),
						rgb_payment,
						PathBuf::from(&ldk_data_dir),
					);
				}
				"getinvoice" | "getcoloredinvoice" => {
					let is_colored = word == "getcoloredinvoice";
					let getinvoice_cmd =
						"`getinvoice <amt_msats> <expiry_secs> <rgb_contract_id> <amt_rgb>`";
					let amt_str = words.next();
					let expiry_secs_str = words.next();
					let contract_id_str = words.next();
					let amt_rgb_str = words.next();

					if is_colored
						&& (amt_str.is_none()
							|| expiry_secs_str.is_none() || contract_id_str.is_none()
							|| amt_rgb_str.is_none())
					{
						println!(
							"ERROR: getcoloredinvoice has 4 required arguments: {getinvoice_cmd}"
						);
						continue;
					} else if !is_colored && (amt_str.is_none() || expiry_secs_str.is_none()) {
						println!("ERROR: getinvoice has 2 required arguments: getinvoice <amt_msats> <expiry_secs>");
						continue;
					}

					let amt_msat: Result<u64, _> = amt_str.unwrap().parse();
					if amt_msat.is_err() {
						println!("ERROR: getinvoice provided payment amount was not a number");
						continue;
					}
					let amt_msat = amt_msat.unwrap();
					if amt_msat < INVOICE_MIN_MSAT {
						println!("ERROR: amt_msat cannot be less than {INVOICE_MIN_MSAT}");
						continue;
					}

					let expiry_secs: Result<u32, _> = expiry_secs_str.unwrap().parse();
					if expiry_secs.is_err() {
						println!("ERROR: getinvoice provided expiry was not a number");
						continue;
					}

					let (contract_id, amt_rgb) = if is_colored {
						let contract_id_str = contract_id_str.unwrap();
						let contract_id = match ContractId::from_str(contract_id_str) {
							Ok(cid) => cid,
							Err(_) => {
								println!("ERROR: invalid contract ID: {contract_id_str}");
								continue;
							}
						};

						let amt_rgb: u64 = match amt_rgb_str.unwrap().parse() {
							Ok(amt) => amt,
							Err(e) => {
								println!("ERROR: couldn't parse amt_rgb: {e}");
								continue;
							}
						};

						(Some(contract_id), Some(amt_rgb))
					} else {
						(None, None)
					};

					get_invoice(
						amt_msat,
						Arc::clone(&inbound_payments),
						&*channel_manager,
						Arc::clone(&keys_manager),
						network,
						expiry_secs.unwrap(),
						Arc::clone(&logger),
						contract_id,
						amt_rgb,
					);
				}
				"connectpeer" => {
					let peer_pubkey_and_ip_addr = words.next();
					if peer_pubkey_and_ip_addr.is_none() {
						println!("ERROR: connectpeer requires peer connection info: `connectpeer pubkey@host:port`");
						continue;
					}
					let (pubkey, peer_addr) =
						match parse_peer_info(peer_pubkey_and_ip_addr.unwrap().to_string()) {
							Ok(info) => info,
							Err(e) => {
								println!("{:?}", e.into_inner().unwrap());
								continue;
							}
						};
					if connect_peer_if_necessary(pubkey, peer_addr, peer_manager.clone())
						.await
						.is_ok()
					{
						println!("SUCCESS: connected to peer {}", pubkey);
					}
				}
				"disconnectpeer" => {
					let peer_pubkey = words.next();
					if peer_pubkey.is_none() {
						println!("ERROR: disconnectpeer requires peer public key: `disconnectpeer <peer_pubkey>`");
						continue;
					}

					let peer_pubkey =
						match bitcoin::secp256k1::PublicKey::from_str(peer_pubkey.unwrap()) {
							Ok(pubkey) => pubkey,
							Err(e) => {
								println!("ERROR: {}", e.to_string());
								continue;
							}
						};

					if do_disconnect_peer(
						peer_pubkey,
						peer_manager.clone(),
						channel_manager.clone(),
					)
					.is_ok()
					{
						println!("SUCCESS: disconnected from peer {}", peer_pubkey);
					}
				}
				"listchannels" => {
					list_channels(&channel_manager, &network_graph, ldk_data_dir.clone())
				}
				// Called by the service/exchange to initiate the trade. The swaptype is seen from
				// the poin of view of the user, so "buy" means that the user (=taker) is buying
				// assets for bitcoin, and sell that the user is selling assets for bitcoin
				"makerinit" => {
					let amt_asset = words.next();
					let asset_id = words.next();
					let swaptype = words.next();
					let timeout = words.next();
					let price = words.next();

					if timeout.is_none() {
						println!("ERROR: makerinit requires at least 4 args: makerinit <amount> <asset_id> <buy|sell> <expiry_secs> [<price_msats_per_asset>]");
						continue;
					}

					let asset_id = match ContractId::from_str(asset_id.unwrap()) {
						Ok(cid) => cid,
						Err(_) => {
							println!("ERROR: invalid contract ID: {}", asset_id.unwrap());
							continue;
						}
					};
					let timeout = match timeout.unwrap().parse() {
						Ok(t) if t > 0 => t,
						Ok(_) | Err(_) => {
							println!("ERROR: Invalid expiry_secs value");
							continue;
						}
					};
					let price = match price {
						Some(x) if !x.trim().is_empty() => x.parse::<u64>().map_err(|_| {
							println!("ERROR: invalid price_msats_per_asset");
							()
						}),
						_ => fetch_price(&asset_id).await.map_err(|e| {
							println!("ERROR: invalid price_msats_per_asset: {}", e);
							()
						}),
					};
					if price.is_err() {
						continue;
					}
					let price = price.unwrap();
					if price == 0 {
						println!("ERROR: invalid price_msats_per_asset");
						continue;
					}
					let amt_asset = match amt_asset.unwrap().parse::<u64>() {
						Ok(amt) if amt > 0 => amt,
						Ok(_) | Err(_) => {
							println!("ERROR: invalid amt_asset");
							continue;
						}
					};
					let swaptype = match swaptype {
						Some("buy") => SwapType::BuyAsset {
							amount_rgb: amt_asset,
							amount_msats: amt_asset * price,
						},
						Some("sell") => SwapType::SellAsset {
							amount_rgb: amt_asset,
							amount_msats: amt_asset * price,
						},
						_ => {
							println!("ERROR: invalid swap type, use either `buy` or `sell`");
							continue;
						}
					};

					// The user is buying assets = we (the service) are selling assets. Do we have
					// enough?
					if swaptype.is_buy() {
						let runtime = get_rgb_runtime(&PathBuf::from(ldk_data_dir.clone()));

						let total_rgb_amount = match get_rgb_total_amount(
							asset_id,
							&runtime,
							wallet_arc.clone(),
							electrum_url.clone(),
						) {
							Ok(a) => a,
							Err(e) => {
								println!("{e}");
								continue;
							}
						};
						drop(runtime);
						drop_rgb_runtime(&PathBuf::from(ldk_data_dir.clone()));

						if amt_asset > total_rgb_amount {
							println!("ERROR: do not have enough RGB assets");
							continue;
						}
					}

					let (payment_hash, payment_secret) =
						maker_init(&channel_manager, &swaptype, timeout, asset_id, &maker_trades);
					let expiry = get_current_timestamp() + timeout as u64;
					let swapstring = format!(
						"{}:{}:{}:{}:{}:{}",
						amt_asset,
						asset_id,
						swaptype.side(),
						price,
						expiry,
						hex_utils::hex_str(&payment_hash.0)
					);
					println!("SUCCESS! swap_string = {}", swapstring);

					let payment_secret = hex_utils::hex_str(&payment_secret.0);
					println!("payment_secret: {}", payment_secret);
					println!(
						"To execute the swap run `makerexecute {} {} <peer_pubkey>`",
						swapstring, payment_secret
					);
				}
				"taker" => {
					let swapstring = words.next();

					if swapstring.is_none() {
						println!("ERROR: taker requires 1 arg: taker <swap_string>");
						continue;
					}

					let swapstring = match SwapString::from_str(swapstring.unwrap()) {
						Ok(v) => v,
						Err(e) => {
							println!("ERROR: {}", e);
							continue;
						}
					};

					if get_current_timestamp() > swapstring.expiry {
						println!("ERROR: the swap offer has already expired");
						continue;
					}

					// We are selling assets, do we have enough?
					if !swapstring.swap_type.is_buy() {
						let runtime = get_rgb_runtime(&PathBuf::from(ldk_data_dir.clone()));

						let total_rgb_amount = match get_rgb_total_amount(
							swapstring.asset_id,
							&runtime,
							wallet_arc.clone(),
							electrum_url.clone(),
						) {
							Ok(a) => a,
							Err(e) => {
								println!("{e}");
								continue;
							}
						};
						drop(runtime);
						drop_rgb_runtime(&PathBuf::from(ldk_data_dir.clone()));

						if swapstring.swap_type.amount_rgb() > total_rgb_amount {
							println!("ERROR: do not have enough RGB assets");
							continue;
						}
					}

					whitelisted_trades.lock().unwrap().insert(
						swapstring.payment_hash,
						(swapstring.asset_id, swapstring.swap_type),
					);
					println!("SUCCESS: Trade whitelisted!");
					println!("our_pk: {}", channel_manager.get_our_node_id());
				}
				"tradeslist" => {
					let filter = words.next();

					let print_trade = |(k, v): (&PaymentHash, &(ContractId, SwapType))| {
						println!("\t\t{{");

						println!("\t\t\tpayment_hash: {}", hex_utils::hex_str(&k.0));
						println!("\t\t\tcontract_id: {}", v.0);
						println!("\t\t\tside: {}", v.1.side());
						println!("\t\t\tamount_msats: {}", v.1.amount_msats());
						println!("\t\t\tamount_rgb: {}", v.1.amount_rgb());

						println!("\t\t}},");
					};

					println!("{{");
					if filter.is_none() || filter == Some("taker") {
						let lock = whitelisted_trades.lock().unwrap();

						println!("\ttaker: [");
						for tuple in lock.iter() {
							print_trade(tuple);
						}
						println!("\t],");
					}
					if filter.is_none() || filter == Some("maker") {
						let lock = maker_trades.lock().unwrap();

						println!("\tmaker: [");
						for tuple in lock.iter() {
							print_trade(tuple);
						}
						println!("\t],");
					}
					println!("}}");
				}
				"makerexecute" => {
					let swapstring = words.next();
					let payment_secret = words.next();
					let peer_pubkey = words.next();

					if peer_pubkey.is_none() {
						println!("ERROR: makerexecute requires 3 args: makerexecute <swap_string> <payment_secret> <peer_pubkey>");
						continue;
					}

					let swapstring = match SwapString::from_str(swapstring.unwrap()) {
						Ok(v) => v,
						Err(e) => {
							println!("ERROR: {}", e);
							continue;
						}
					};

					if get_current_timestamp() > swapstring.expiry {
						println!("ERROR: the swap offer has already expired");
						continue;
					}

					let payment_secret = hex_utils::to_vec(payment_secret.unwrap())
						.and_then(|vec| vec.try_into().ok())
						.map(|slice| PaymentSecret(slice));
					let payment_secret = match payment_secret {
						Some(v) => v,
						_ => {
							println!("ERROR: invalid payment hash");
							continue;
						}
					};
					let peer_pubkey =
						match bitcoin::secp256k1::PublicKey::from_str(peer_pubkey.unwrap()) {
							Ok(pubkey) => pubkey,
							Err(e) => {
								println!("ERROR: {}", e.to_string());
								continue;
							}
						};

					maker_execute(
						&channel_manager,
						&router,
						peer_pubkey,
						swapstring.swap_type,
						swapstring.asset_id,
						swapstring.payment_hash,
						payment_secret,
						outbound_payments.clone(),
						PathBuf::from(&ldk_data_dir),
					);
				}
				"listpayments" => {
					list_payments(inbound_payments.clone(), outbound_payments.clone())
				}
				"invoicestatus" => {
					let invoice = words.next();
					if invoice.is_none() {
						println!(
							"ERROR: invoicestatus requires an invoice: `invoicestatus <invoice>`"
						);
						continue;
					};
					let invoice = match Invoice::from_str(invoice.unwrap()) {
						Err(e) => {
							println!("ERROR: invalid invoice: {:?}", e);
							continue;
						}
						Ok(v) => v,
					};

					invoice_status(inbound_payments.clone(), invoice)
				}
				"closechannel" => {
					let channel_id_str = words.next();
					if channel_id_str.is_none() {
						println!("ERROR: closechannel requires a channel ID: `closechannel <channel_id> <peer_pubkey>`");
						continue;
					}
					let channel_id_vec = hex_utils::to_vec(channel_id_str.unwrap());
					if channel_id_vec.is_none() || channel_id_vec.as_ref().unwrap().len() != 32 {
						println!("ERROR: couldn't parse channel_id");
						continue;
					}
					let mut channel_id = [0; 32];
					channel_id.copy_from_slice(&channel_id_vec.unwrap());

					let peer_pubkey_str = words.next();
					if peer_pubkey_str.is_none() {
						println!("ERROR: closechannel requires a peer pubkey: `closechannel <channel_id> <peer_pubkey>`");
						continue;
					}
					let peer_pubkey_vec = match hex_utils::to_vec(peer_pubkey_str.unwrap()) {
						Some(peer_pubkey_vec) => peer_pubkey_vec,
						None => {
							println!("ERROR: couldn't parse peer_pubkey");
							continue;
						}
					};
					let peer_pubkey = match PublicKey::from_slice(&peer_pubkey_vec) {
						Ok(peer_pubkey) => peer_pubkey,
						Err(_) => {
							println!("ERROR: couldn't parse peer_pubkey");
							continue;
						}
					};

					close_channel(channel_id, peer_pubkey, channel_manager.clone());
				}
				"forceclosechannel" => {
					let channel_id_str = words.next();
					if channel_id_str.is_none() {
						println!("ERROR: forceclosechannel requires a channel ID: `forceclosechannel <channel_id> <peer_pubkey>`");
						continue;
					}
					let channel_id_vec = hex_utils::to_vec(channel_id_str.unwrap());
					if channel_id_vec.is_none() || channel_id_vec.as_ref().unwrap().len() != 32 {
						println!("ERROR: couldn't parse channel_id");
						continue;
					}
					let mut channel_id = [0; 32];
					channel_id.copy_from_slice(&channel_id_vec.unwrap());

					let peer_pubkey_str = words.next();
					if peer_pubkey_str.is_none() {
						println!("ERROR: forceclosechannel requires a peer pubkey: `forceclosechannel <channel_id> <peer_pubkey>`");
						continue;
					}
					let peer_pubkey_vec = match hex_utils::to_vec(peer_pubkey_str.unwrap()) {
						Some(peer_pubkey_vec) => peer_pubkey_vec,
						None => {
							println!("ERROR: couldn't parse peer_pubkey");
							continue;
						}
					};
					let peer_pubkey = match PublicKey::from_slice(&peer_pubkey_vec) {
						Ok(peer_pubkey) => peer_pubkey,
						Err(_) => {
							println!("ERROR: couldn't parse peer_pubkey");
							continue;
						}
					};

					force_close_channel(channel_id, peer_pubkey, channel_manager.clone());
				}
				"nodeinfo" => node_info(&channel_manager, &peer_manager),
				"listpeers" => list_peers(peer_manager.clone()),
				"signmessage" => {
					const MSG_STARTPOS: usize = "signmessage".len() + 1;
					if line.as_bytes().len() <= MSG_STARTPOS {
						println!("ERROR: signmsg requires a message");
						continue;
					}
					println!(
						"{:?}",
						lightning::util::message_signing::sign(
							&line.as_bytes()[MSG_STARTPOS..],
							&keys_manager.get_node_secret_key()
						)
					);
				}
				"sendonionmessage" => {
					let path_pks_str = words.next();
					if path_pks_str.is_none() {
						println!(
							"ERROR: sendonionmessage requires at least one node id for the path"
						);
						continue;
					}
					let mut node_pks = Vec::new();
					let mut errored = false;
					for pk_str in path_pks_str.unwrap().split(",") {
						let node_pubkey_vec = match hex_utils::to_vec(pk_str) {
							Some(peer_pubkey_vec) => peer_pubkey_vec,
							None => {
								println!("ERROR: couldn't parse peer_pubkey");
								errored = true;
								break;
							}
						};
						let node_pubkey = match PublicKey::from_slice(&node_pubkey_vec) {
							Ok(peer_pubkey) => peer_pubkey,
							Err(_) => {
								println!("ERROR: couldn't parse peer_pubkey");
								errored = true;
								break;
							}
						};
						node_pks.push(node_pubkey);
					}
					if errored {
						continue;
					}
					let tlv_type = match words.next().map(|ty_str| ty_str.parse()) {
						Some(Ok(ty)) if ty >= 64 => ty,
						_ => {
							println!("Need an integral message type above 64");
							continue;
						}
					};
					let data = match words.next().map(|s| hex_utils::to_vec(s)) {
						Some(Some(data)) => data,
						_ => {
							println!("Need a hex data string");
							continue;
						}
					};
					let destination_pk = node_pks.pop().unwrap();
					match onion_messenger.send_onion_message(
						&node_pks,
						Destination::Node(destination_pk),
						OnionMessageContents::Custom(UserOnionMessageContents { tlv_type, data }),
						None,
					) {
						Ok(()) => println!("SUCCESS: forwarded onion message to first hop"),
						Err(e) => println!("ERROR: failed to send onion message: {:?}", e),
					}
				}
				"quit" | "exit" => {
					println!("Exiting node...");
					break;
				}
				_ => println!("Unknown command. See `\"help\" for available commands."),
			}
		}
	}
}

fn help() {
	let package_version = env!("CARGO_PKG_VERSION");
	let package_name = env!("CARGO_PKG_NAME");
	println!("\nVERSION:");
	println!("  {} v{}", package_name, package_version);
	println!("\nUSAGE:");
	println!("  Command [arguments]");
	println!("\nCOMMANDS:");
	println!("  help\tShows a list of commands.");
	println!("  quit\tClose the application.");
	println!("\n  Channels:");
	println!("      opencoloredchannel pubkey@host:port <chan_amt_satoshis> <push_amt_msatoshis> <rgb_contract_id> <chan_amt_rgb> [--public]");
	println!(
		"      openchannel pubkey@host:port <chan_amt_satoshis> <push_amt_msatoshis> [--public]"
	);
	println!("      closechannel <channel_id> <peer_pubkey>");
	println!("      forceclosechannel <channel_id> <peer_pubkey>");
	println!("      listchannels");
	println!("\n  Routing:");
	println!("      getroute <dest_pubkey> <amt> [<asset_id>]");
	println!("\n  Swaps:");
	println!(
		"      makerinit <amount> <asset_id> <buy|sell> <expiry_secs> [<price_msats_per_asset>]"
	);
	println!("      taker <swap_string>");
	println!("      tradeslist [<taker|maker>]");
	println!("      makerexecute <payment_hash> <payment_secret> <peer_pubkey>");
	println!("\n  Peers:");
	println!("      connectpeer pubkey@host:port");
	println!("      disconnectpeer <peer_pubkey>");
	println!("      listpeers");
	println!("\n  Payments:");
	println!("      sendpayment <invoice>");
	println!("      coloredkeysend <dest_pubkey> <amt_msats> <rgb_contract_id> <amt_rgb>");
	println!("      keysend <dest_pubkey> <amt_msats>");
	println!("      listpayments");
	println!("\n  Invoices:");
	println!("      getcoloredinvoice <amt_msats> <expiry_secs> <rgb_contract_id> <amt_rgb>");
	println!("      getinvoice <amt_msats> <expiry_secs>");
	println!("      invoicestatus <invoice>");
	println!("\n  Onchain:");
	println!("      getaddress");
	println!("      listunspent");
	println!("\n  RGB:");
	println!("      createutxos");
	println!("      issueasset <supply> <ticker> <name> <precision>");
	println!("      assetbalance <contract_id>");
	println!("      sendasset <rgb_contract_id> <amt_rgb>");
	println!("      receiveasset");
	println!("      refresh");
	println!("\n  Other:");
	println!("      mine <num_blocks>");
	println!("      signmessage <message>");
	println!(
		"      sendonionmessage <node_id_1,node_id_2,..,destination_node_id> <type> <hex_bytes>"
	);
	println!("      nodeinfo");
}

fn node_info(channel_manager: &Arc<ChannelManager>, peer_manager: &Arc<PeerManager>) {
	println!("\t{{");
	println!("\t\t node_pubkey: {}", channel_manager.get_our_node_id());
	let chans = channel_manager.list_channels();
	println!("\t\t num_channels: {}", chans.len());
	println!("\t\t num_usable_channels: {}", chans.iter().filter(|c| c.is_usable).count());
	let local_balance_msat = chans.iter().map(|c| c.balance_msat).sum::<u64>();
	println!("\t\t local_balance_msat: {}", local_balance_msat);
	println!("\t\t num_peers: {}", peer_manager.get_peer_node_ids().len());
	println!("\t}},");
}

fn list_peers(peer_manager: Arc<PeerManager>) {
	println!("\t{{");
	for (pubkey, _) in peer_manager.get_peer_node_ids() {
		println!("\t\t pubkey: {}", pubkey);
	}
	println!("\t}},");
}

fn list_channels(
	channel_manager: &Arc<ChannelManager>, network_graph: &Arc<NetworkGraph>, ldk_data_dir: String,
) {
	print!("[");
	for chan_info in channel_manager.list_channels() {
		println!("");
		println!("\t{{");
		println!("\t\tchannel_id: {},", hex_utils::hex_str(&chan_info.channel_id[..]));
		if let Some(funding_txo) = chan_info.funding_txo {
			println!("\t\tfunding_txid: {},", funding_txo.txid);
		}

		println!(
			"\t\tpeer_pubkey: {},",
			hex_utils::hex_str(&chan_info.counterparty.node_id.serialize())
		);
		if let Some(node_info) = network_graph
			.read_only()
			.nodes()
			.get(&NodeId::from_pubkey(&chan_info.counterparty.node_id))
		{
			if let Some(announcement) = &node_info.announcement_info {
				println!("\t\tpeer_alias: {}", announcement.alias);
			}
		}

		if let Some(id) = chan_info.short_channel_id {
			println!("\t\tshort_channel_id: {},", id);
		}
		println!("\t\tis_channel_ready: {},", chan_info.is_channel_ready);
		println!("\t\tchannel_value_satoshis: {},", chan_info.channel_value_satoshis);
		println!("\t\tlocal_balance_msat: {},", chan_info.balance_msat);
		if chan_info.is_usable {
			println!("\t\tavailable_balance_for_send_msat: {},", chan_info.outbound_capacity_msat);
			println!("\t\tavailable_balance_for_recv_msat: {},", chan_info.inbound_capacity_msat);
		}
		println!("\t\tchannel_can_send_payments: {},", chan_info.is_usable);
		println!("\t\tpublic: {},", chan_info.is_public);

		let ldk_data_dir_path = PathBuf::from(ldk_data_dir.clone());
		let info_file_path = ldk_data_dir_path.join(hex::encode(chan_info.channel_id));
		let (contract_id, local_rgb_amount, remote_rgb_amount) = if info_file_path.exists() {
			let (rgb_info, _) = get_rgb_channel_info(&chan_info.channel_id, &ldk_data_dir_path);
			(
				rgb_info.contract_id.to_string(),
				rgb_info.local_rgb_amount.to_string(),
				rgb_info.remote_rgb_amount.to_string(),
			)
		} else {
			let not_available = "N/A".to_string();
			(not_available.clone(), not_available.clone(), not_available)
		};
		println!("\t\trgb_contract_id: {contract_id},");
		println!("\t\trgb_local_amount: {local_rgb_amount},");
		println!("\t\trgb_remote_amount: {remote_rgb_amount},");
		println!("\t}},");
	}
	println!("]");
}

fn invoice_status(inbound_payments: PaymentInfoStorage, invoice: Invoice) {
	let inbound = inbound_payments.lock().unwrap();

	let payment_hash = PaymentHash(invoice.payment_hash().into_inner());
	match inbound.get(&payment_hash) {
		Some(v) => {
			let status_str = match v.status {
				HTLCStatus::Pending if invoice.is_expired() => "expired",
				HTLCStatus::Pending => "pending",
				HTLCStatus::Succeeded => "succeeded",
				HTLCStatus::Failed => "failed",
			};
			println!("{}", status_str);
		}
		None => {
			println!("ERROR: unknown invoice");
		}
	};
}

fn list_payments(inbound_payments: PaymentInfoStorage, outbound_payments: PaymentInfoStorage) {
	let inbound = inbound_payments.lock().unwrap();
	let outbound = outbound_payments.lock().unwrap();
	print!("[");
	for (payment_hash, payment_info) in inbound.deref() {
		println!("");
		println!("\t{{");
		println!("\t\tamount_millisatoshis: {},", payment_info.amt_msat);
		println!("\t\tpayment_hash: {},", hex_utils::hex_str(&payment_hash.0));
		println!("\t\thtlc_direction: inbound,");
		println!(
			"\t\thtlc_status: {},",
			match payment_info.status {
				HTLCStatus::Pending => "pending",
				HTLCStatus::Succeeded => "succeeded",
				HTLCStatus::Failed => "failed",
			}
		);

		println!("\t}},");
	}

	for (payment_hash, payment_info) in outbound.deref() {
		println!("");
		println!("\t{{");
		println!("\t\tamount_millisatoshis: {},", payment_info.amt_msat);
		println!("\t\tpayment_hash: {},", hex_utils::hex_str(&payment_hash.0));
		println!("\t\thtlc_direction: outbound,");
		println!(
			"\t\thtlc_status: {},",
			match payment_info.status {
				HTLCStatus::Pending => "pending",
				HTLCStatus::Succeeded => "succeeded",
				HTLCStatus::Failed => "failed",
			}
		);

		println!("\t}},");
	}
	println!("]");
}

fn get_route(
	channel_manager: &ChannelManager, router: &Router, start: PublicKey, dest: PublicKey,
	asset_id: Option<ContractId>, final_value_msat: Option<u64>, hints: Vec<RouteHint>,
) -> Option<Route> {
	let inflight_htlcs = channel_manager.compute_inflight_htlcs();
	let payment_params = PaymentParameters {
		payee_pubkey: dest,
		features: None,
		route_hints: Hints::Clear(hints),
		expiry_time: None,
		max_total_cltv_expiry_delta: DEFAULT_MAX_TOTAL_CLTV_EXPIRY_DELTA,
		max_path_count: 1,
		max_channel_saturation_power_of_half: 2,
		previously_failed_channels: vec![],
		final_cltv_expiry_delta: 14,
	};
	let route = router.find_route(
		&start,
		&RouteParameters {
			payment_params,
			final_value_msat: final_value_msat.unwrap_or(HTLC_MIN_MSAT),
		},
		None,
		&inflight_htlcs,
		asset_id,
	);

	route.ok()
}

fn maker_init(
	channel_manager: &ChannelManager, swaptype: &SwapType, timeout_secs: u32,
	contract_id: ContractId,
	maker_trades: &Arc<Mutex<HashMap<PaymentHash, (ContractId, SwapType)>>>,
) -> (PaymentHash, PaymentSecret) {
	let (payment_hash, payment_secret) = channel_manager
		.create_inbound_payment(Some(swaptype.amount_msats()), timeout_secs, None)
		.unwrap();
	maker_trades.lock().unwrap().insert(payment_hash, (contract_id, *swaptype));

	(payment_hash, payment_secret)
}

fn maker_execute(
	channel_manager: &ChannelManager, router: &Router, taker_pk: PublicKey, swaptype: SwapType,
	asset_id: ContractId, payment_hash: PaymentHash, payment_secret: PaymentSecret,
	payment_storage: PaymentInfoStorage, ldk_data_dir: PathBuf,
) {
	let payment_preimage = match channel_manager.get_payment_preimage(payment_hash, payment_secret)
	{
		Ok(p) => p,
		Err(e) => {
			println!("ERROR: unable to find payment preimage, have you provided the correct swapstring and payment secret? {:?}", e);
			return;
		}
	};
	// The cli takes the swaptype from the point of view of the user (=who wants to do the trade)
	// In this function we're the market maker (=who sends the payment, completing the user trade)
	// We swap the swaptype to hopefully make this function easier to read
	let swaptype = swaptype.opposite();

	let receive_hints = channel_manager
		.list_usable_channels()
		.iter()
		.map(|details| {
			let config = details.counterparty.forwarding_info.as_ref().unwrap();
			RouteHint(vec![RouteHintHop {
				src_node_id: details.counterparty.node_id,
				short_channel_id: details.short_channel_id.unwrap(),
				cltv_expiry_delta: config.cltv_expiry_delta,
				htlc_maximum_msat: None,
				htlc_minimum_msat: None,
				fees: RoutingFees {
					base_msat: config.fee_base_msat,
					proportional_millionths: config.fee_proportional_millionths,
				},
			}])
		})
		.collect();

	let first_leg = get_route(
		channel_manager,
		router,
		channel_manager.get_our_node_id(),
		taker_pk,
		if swaptype.is_buy() { None } else { Some(asset_id) },
		if swaptype.is_buy() { Some(swaptype.amount_msats()) } else { None },
		vec![],
	);
	let second_leg = get_route(
		channel_manager,
		router,
		taker_pk,
		channel_manager.get_our_node_id(),
		if swaptype.is_buy() { Some(asset_id) } else { None },
		if swaptype.is_buy() { Some(HTLC_MIN_MSAT) } else { Some(swaptype.amount_msats()) },
		receive_hints,
	);

	let (mut first_leg, mut second_leg) = match (first_leg, second_leg) {
		(Some(f), Some(s)) => (f, s),
		(Some(_), _) => {
			println!("ERROR: unable to find from the taker to us");
			return;
		}
		(_, Some(_)) => {
			println!("ERROR: unable to find path to the taker");
			return;
		}
		_ => {
			println!("ERROR: no path found");
			return;
		}
	};

	// Set swap flag
	second_leg.paths[0].hops[0].short_channel_id |= IS_SWAP_SCID;

	// Generally in the last hop the fee_amount is set to the payment amount, so we need to
	// override it depending on what type of swap we are doing
	if let SwapType::BuyAsset { .. } = swaptype {
		first_leg.paths[0].hops.last_mut().expect("Path not to be empty").fee_msat = HTLC_MIN_MSAT;
	} else {
		first_leg.paths[0].hops.last_mut().expect("Path not to be empty").fee_msat = 0;
	}

	let fullpaths = first_leg.paths[0]
		.hops
		.clone()
		.into_iter()
		.map(|mut hop| {
			if let SwapType::SellAsset { amount_rgb, .. } = swaptype {
				hop.rgb_amount = Some(amount_rgb);
				hop.payment_amount = HTLC_MIN_MSAT;
			}
			hop
		})
		.chain(second_leg.paths[0].hops.clone().into_iter().map(|mut hop| {
			if let SwapType::BuyAsset { amount_rgb, .. } = swaptype {
				hop.rgb_amount = Some(amount_rgb);
				hop.payment_amount = HTLC_MIN_MSAT;
			}
			hop
		}))
		.collect::<Vec<_>>();

	let route = Route {
		paths: vec![LnPath { hops: fullpaths, blinded_tail: None }],
		payment_params: Some(PaymentParameters::for_keysend(channel_manager.get_our_node_id(), 40)),
	};

	if let SwapType::SellAsset { amount_rgb, .. } = swaptype {
		write_rgb_payment_info_file(&ldk_data_dir, &payment_hash, asset_id, amount_rgb, None);
	}

	let status = match channel_manager.send_spontaneous_payment(
		&route,
		Some(payment_preimage),
		RecipientOnionFields::spontaneous_empty(),
		PaymentId(payment_hash.0),
	) {
		Ok(_payment_hash) => {
			println!("EVENT: initiated swap");
			print!("> ");
			HTLCStatus::Pending
		}
		Err(e) => {
			println!("ERROR: failed to send payment: {:?}", e);
			print!("> ");
			HTLCStatus::Failed
		}
	};

	let mut payments = payment_storage.lock().unwrap();
	payments.insert(
		payment_hash,
		PaymentInfo {
			preimage: None,
			secret: None,
			status,
			amt_msat: MillisatAmount(Some(swaptype.amount_msats())),
		},
	);
}

/// Return the price in msats/asset
async fn fetch_price(asset_id: &ContractId) -> Result<u64, Box<dyn std::error::Error>> {
	#[derive(Debug, serde::Deserialize)]
	struct BitfinexPrice {
		last_price: String,
	}

	// TODO: map the asset to the right ticker. here we assume it's always USDt
	// See: https://github.com/RGB-Tools/rgb-lightning-sample/issues/6
	let body = reqwest::get("https://api.bitfinex.com/v1/pubticker/btcust")
		.await?
		.json::<BitfinexPrice>()
		.await?;

	let last_price = body.last_price.parse::<f64>()?;
	if last_price == 0.0 {
		return Err(Box::<dyn std::error::Error>::from(
			"BTC/USDt price fetched from bitfinex is zero",
		));
	}
	let price = (1.0 / last_price * 1e11) as u64;

	println!("Using price from Bitfinex: {} mSAT = 1 {}", price, asset_id);

	Ok(price)
}

pub(crate) async fn connect_peer_if_necessary(
	pubkey: PublicKey, peer_addr: SocketAddr, peer_manager: Arc<PeerManager>,
) -> Result<(), ()> {
	for (node_pubkey, _) in peer_manager.get_peer_node_ids() {
		if node_pubkey == pubkey {
			return Ok(());
		}
	}
	let res = do_connect_peer(pubkey, peer_addr, peer_manager).await;
	if res.is_err() {
		println!("ERROR: failed to connect to peer");
	}
	res
}

pub(crate) async fn do_connect_peer(
	pubkey: PublicKey, peer_addr: SocketAddr, peer_manager: Arc<PeerManager>,
) -> Result<(), ()> {
	match lightning_net_tokio::connect_outbound(Arc::clone(&peer_manager), pubkey, peer_addr).await
	{
		Some(connection_closed_future) => {
			let mut connection_closed_future = Box::pin(connection_closed_future);
			loop {
				tokio::select! {
					_ = &mut connection_closed_future => return Err(()),
					_ = tokio::time::sleep(Duration::from_millis(10)) => {},
				};
				if peer_manager.get_peer_node_ids().iter().find(|(id, _)| *id == pubkey).is_some() {
					return Ok(());
				}
			}
		}
		None => Err(()),
	}
}

fn do_disconnect_peer(
	pubkey: bitcoin::secp256k1::PublicKey, peer_manager: Arc<PeerManager>,
	channel_manager: Arc<ChannelManager>,
) -> Result<(), ()> {
	//check for open channels with peer
	for channel in channel_manager.list_channels() {
		if channel.counterparty.node_id == pubkey {
			println!("Error: Node has an active channel with this peer, close any channels first");
			return Err(());
		}
	}

	//check the pubkey matches a valid connected peer
	let peers = peer_manager.get_peer_node_ids();
	if !peers.iter().any(|(pk, _)| &pubkey == pk) {
		println!("Error: Could not find peer {}", pubkey);
		return Err(());
	}

	peer_manager.disconnect_by_node_id(pubkey);
	Ok(())
}

fn open_channel(
	peer_pubkey: PublicKey, channel_amt_sat: u64, push_amt_msat: u64, announced_channel: bool,
	channel_manager: Arc<ChannelManager>, proxy_endpoint: &str, is_colored: bool,
	wallet: Arc<Mutex<Wallet<SqliteDatabase>>>, electrum_url: &str,
) -> Result<[u8; 32], ()> {
	{
		let wallet = wallet.lock().unwrap();
		sync_wallet(&wallet, electrum_url.to_string());
	}

	let config = UserConfig {
		channel_handshake_limits: ChannelHandshakeLimits {
			// lnd's max to_self_delay is 2016, so we want to be compatible.
			their_to_self_delay: 2016,
			..Default::default()
		},
		channel_handshake_config: ChannelHandshakeConfig {
			announced_channel,
			our_htlc_minimum_msat: HTLC_MIN_MSAT,
			..Default::default()
		},
		..Default::default()
	};

	let consignment_endpoint =
		if is_colored { Some(RgbTransport::from_str(proxy_endpoint).unwrap()) } else { None };

	match channel_manager.create_channel(
		peer_pubkey,
		channel_amt_sat,
		push_amt_msat,
		0,
		Some(config),
		consignment_endpoint,
	) {
		Ok(temporary_channel_id) => {
			println!("EVENT: initiated channel with peer {}. ", peer_pubkey);
			return Ok(temporary_channel_id);
		}
		Err(e) => {
			println!("ERROR: failed to open channel: {:?}", e);
			return Err(());
		}
	}
}

fn send_payment(
	channel_manager: &ChannelManager, invoice: &Invoice, payment_storage: PaymentInfoStorage,
	ldk_data_dir: PathBuf,
) {
	let payment_hash = PaymentHash(invoice.payment_hash().clone().into_inner());
	if let Some(contract_id) = invoice.rgb_contract_id() {
		write_rgb_payment_info_file(
			&ldk_data_dir,
			&payment_hash,
			contract_id,
			invoice.rgb_amount().unwrap(),
			Some(invoice.rgb_amount().unwrap()),
		);
	}
	let status =
		match pay_invoice(invoice, Retry::Timeout(Duration::from_secs(10)), channel_manager) {
			Ok(_payment_id) => {
				let payee_pubkey = invoice.recover_payee_pub_key();
				let amt_msat = invoice.amount_milli_satoshis().unwrap();
				println!("EVENT: initiated sending {} msats to {}", amt_msat, payee_pubkey);
				print!("> ");
				HTLCStatus::Pending
			}
			Err(e) => {
				println!("ERROR: failed to send payment: {:?}", e);
				print!("> ");
				HTLCStatus::Failed
			}
		};
	let payment_secret = Some(invoice.payment_secret().clone());

	let mut payments = payment_storage.lock().unwrap();
	payments.insert(
		payment_hash,
		PaymentInfo {
			preimage: None,
			secret: payment_secret,
			status,
			amt_msat: MillisatAmount(invoice.amount_milli_satoshis()),
		},
	);
}

fn keysend<E: EntropySource>(
	channel_manager: &ChannelManager, payee_pubkey: PublicKey, amt_msat: u64, entropy_source: &E,
	payment_storage: PaymentInfoStorage, rgb_payment: Option<(ContractId, u64)>,
	ldk_data_dir: PathBuf,
) {
	let payment_preimage = PaymentPreimage(entropy_source.get_secure_random_bytes());
	let payment_hash = PaymentHash(Sha256::hash(&payment_preimage.0[..]).into_inner());
	if let Some((contract_id, amt_rgb)) = rgb_payment {
		write_rgb_payment_info_file(
			&ldk_data_dir,
			&payment_hash,
			contract_id,
			amt_rgb,
			Some(amt_rgb),
		);
	}

	let route_params = RouteParameters {
		payment_params: PaymentParameters::for_keysend(payee_pubkey, 40),
		final_value_msat: amt_msat,
	};
	let status = match channel_manager.send_spontaneous_payment_with_retry(
		Some(payment_preimage),
		RecipientOnionFields::spontaneous_empty(),
		PaymentId(payment_hash.0),
		route_params,
		Retry::Timeout(Duration::from_secs(10)),
	) {
		Ok(_payment_hash) => {
			println!("EVENT: initiated sending {} msats to {}", amt_msat, payee_pubkey);
			print!("> ");
			HTLCStatus::Pending
		}
		Err(e) => {
			println!("ERROR: failed to send payment: {:?}", e);
			print!("> ");
			HTLCStatus::Failed
		}
	};

	let mut payments = payment_storage.lock().unwrap();
	payments.insert(
		payment_hash,
		PaymentInfo {
			preimage: None,
			secret: None,
			status,
			amt_msat: MillisatAmount(Some(amt_msat)),
		},
	);
}

fn get_invoice(
	amt_msat: u64, payment_storage: PaymentInfoStorage, channel_manager: &ChannelManager,
	keys_manager: Arc<KeysManager>, network: Network, expiry_secs: u32,
	logger: Arc<disk::FilesystemLogger>, contract_id: Option<ContractId>, amt_rgb: Option<u64>,
) {
	let mut payments = payment_storage.lock().unwrap();
	let currency = match network {
		Network::Bitcoin => Currency::Bitcoin,
		Network::Testnet => Currency::BitcoinTestnet,
		Network::Regtest => Currency::Regtest,
		Network::Signet => Currency::Signet,
	};
	let invoice = match utils::create_invoice_from_channelmanager(
		channel_manager,
		keys_manager,
		logger,
		currency,
		Some(amt_msat),
		"ldk-tutorial-node".to_string(),
		expiry_secs,
		None,
		contract_id,
		amt_rgb,
	) {
		Ok(inv) => {
			println!("SUCCESS: generated invoice: {}", inv);
			inv
		}
		Err(e) => {
			println!("ERROR: failed to create invoice: {:?}", e);
			return;
		}
	};

	let payment_hash = PaymentHash(invoice.payment_hash().clone().into_inner());
	payments.insert(
		payment_hash,
		PaymentInfo {
			preimage: None,
			secret: Some(invoice.payment_secret().clone()),
			status: HTLCStatus::Pending,
			amt_msat: MillisatAmount(Some(amt_msat)),
		},
	);
}

fn close_channel(
	channel_id: [u8; 32], counterparty_node_id: PublicKey, channel_manager: Arc<ChannelManager>,
) {
	match channel_manager.close_channel(&channel_id, &counterparty_node_id) {
		Ok(()) => println!("EVENT: initiating channel close"),
		Err(e) => println!("ERROR: failed to close channel: {:?}", e),
	}
}

fn force_close_channel(
	channel_id: [u8; 32], counterparty_node_id: PublicKey, channel_manager: Arc<ChannelManager>,
) {
	match channel_manager.force_close_broadcasting_latest_txn(&channel_id, &counterparty_node_id) {
		Ok(()) => println!("EVENT: initiating channel force-close"),
		Err(e) => println!("ERROR: failed to force-close channel: {:?}", e),
	}
}

pub(crate) fn parse_peer_info(
	peer_pubkey_and_ip_addr: String,
) -> Result<(PublicKey, SocketAddr), std::io::Error> {
	let mut pubkey_and_addr = peer_pubkey_and_ip_addr.split("@");
	let pubkey = pubkey_and_addr.next();
	let peer_addr_str = pubkey_and_addr.next();
	if peer_addr_str.is_none() {
		return Err(std::io::Error::new(
			std::io::ErrorKind::Other,
			"ERROR: incorrectly formatted peer info. Should be formatted as: `pubkey@host:port`",
		));
	}

	let peer_addr = peer_addr_str.unwrap().to_socket_addrs().map(|mut r| r.next());
	if peer_addr.is_err() || peer_addr.as_ref().unwrap().is_none() {
		return Err(std::io::Error::new(
			std::io::ErrorKind::Other,
			"ERROR: couldn't parse pubkey@host:port into a socket address",
		));
	}

	let pubkey = hex_utils::to_compressed_pubkey(pubkey.unwrap());
	if pubkey.is_none() {
		return Err(std::io::Error::new(
			std::io::ErrorKind::Other,
			"ERROR: unable to parse given pubkey for node",
		));
	}

	Ok((pubkey.unwrap(), peer_addr.unwrap().unwrap()))
}

pub(crate) async fn check_uncolored_utxos(ldk_data_dir: &str) -> Result<(), Error> {
	let rgb_utxos_path = format!("{}/rgb_utxos", ldk_data_dir);
	let serialized_utxos = fs::read_to_string(rgb_utxos_path).expect("able to read rgb utxos file");
	let rgb_utxos: RgbUtxos = serde_json::from_str(&serialized_utxos).expect("valid rgb utxos");
	let utxo = rgb_utxos.utxos.iter().find(|u| !u.colored);
	match utxo {
		Some(_) => Ok(()),
		None => Err(Error::NoAvailableUtxos),
	}
}

pub(crate) async fn get_utxo(ldk_data_dir: &str) -> RgbUtxo {
	let rgb_utxos_path = format!("{}/rgb_utxos", ldk_data_dir);
	let serialized_utxos = fs::read_to_string(rgb_utxos_path).expect("able to read rgb utxos file");
	let rgb_utxos: RgbUtxos = serde_json::from_str(&serialized_utxos).expect("valid rgb utxos");
	rgb_utxos.utxos.into_iter().find(|u| !u.colored).expect("at least one unlocked UTXO")
}

pub(crate) async fn mine(bitcoind_client: &BitcoindClient, num_blocks: u16) {
	let address = bitcoind_client.get_new_address().await.to_string();
	bitcoind_client.generate_to_adress(num_blocks, address).await;
}
