use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::{Arc, Mutex};

use amplify_num::hex::FromHex;
use bdk::bitcoin::OutPoint;
use bdk::database::SqliteDatabase;
use bdk::Wallet;
use bitcoin::consensus::{deserialize, serialize};
use bitcoin::hashes::Hash;
use bitcoin::psbt::serialize::Deserialize as BitcoinDeserialize;
use bitcoin::psbt::PartiallySignedTransaction;
use bitcoin::BlockHash;
use bp::seals::txout::CloseMethod;
use internet2::addr::ServiceAddr;
use lnpbp::chain::{Chain, GENESIS_HASH_REGTEST};
use psbt::Psbt;
use rgb::fungible::allocation::{AllocatedValue, OutpointValue};
use rgb::psbt::RgbExt;
use rgb::psbt::RgbInExt;
use rgb::IntoRevealedSeal;
use rgb::Node;
use rgb::{
	AssignedState, Contract, ContractId, EndpointValueMap, InmemConsignment, TransferConsignment,
};
use rgb20::{Asset as Rgb20Asset, Rgb20};
use rgb_rpc::client::Client;
use std::collections::{BTreeMap, BTreeSet};
use std::convert::TryFrom;
use std::str::FromStr;
use stens::AsciiString;

use crate::bdk_utils::sync_wallet;
use crate::error::Error;

pub(crate) fn get_rgb_node_client(port: u16) -> Client {
	let ip = Ipv4Addr::new(127, 0, 0, 1);
	let rgb_node_endpoint = ServiceAddr::Tcp(SocketAddr::V4(SocketAddrV4::new(ip, port)));
	let rgb_network =
		Chain::Regtest(BlockHash::from_slice(GENESIS_HASH_REGTEST).expect("valid bloch hash"));
	Client::with(rgb_node_endpoint, "rgb-ln-node".to_string(), rgb_network)
		.expect("Error initializing client")
}

pub(crate) fn get_rgb_total_amount(
	contract_id: ContractId, rgb_node_client: Arc<Mutex<Client>>,
	wallet_arc: Arc<Mutex<Wallet<SqliteDatabase>>>,
) -> Result<u64, Error> {
	let asset_owned_values = get_asset_owned_values(contract_id, rgb_node_client, wallet_arc)?;
	Ok(asset_owned_values.iter().map(|ov| ov.state.value).sum())
}

pub(crate) fn get_asset_owned_values(
	contract_id: ContractId, rgb_node_client: Arc<Mutex<Client>>,
	wallet_arc: Arc<Mutex<Wallet<SqliteDatabase>>>,
) -> Result<Vec<AssignedState<rgb::value::Revealed>>, Error> {
	let mut rgb_client = rgb_node_client.lock().unwrap();
	let contract_state = match rgb_client.contract_state(contract_id) {
		Ok(cs) => cs,
		Err(_e) => return Err(Error::UnknownContractId),
	};
	let wallet = wallet_arc.lock().unwrap();
	sync_wallet(&wallet);
	let unspents_outpoints: Vec<OutPoint> =
		wallet.list_unspent().expect("valid unspent list").iter().map(|u| u.outpoint).collect();
	Ok(contract_state
		.owned_values
		.into_iter()
		.filter(|ov| {
			unspents_outpoints.contains(&OutPoint { txid: ov.seal.txid, vout: ov.seal.vout })
		})
		.collect())
}

pub(crate) trait RgbUtilities {
	fn issue_contract(
		&mut self, amount: u64, outpoint: OutPoint, ticker: AsciiString, name: AsciiString,
		precision: u8,
	) -> ContractId;

	fn send_rgb_internal(
		&mut self, contract_id: ContractId, psbt: &mut Psbt,
		input_outpoints_bt: BTreeSet<OutPoint>, beneficiaries: EndpointValueMap,
		change: Vec<AllocatedValue>,
	) -> (PartiallySignedTransaction, InmemConsignment<TransferConsignment>);

	fn send_rgb(
		&mut self, contract_id: ContractId, psbt: PartiallySignedTransaction,
		input_outpoints_bt: BTreeSet<OutPoint>, beneficiaries: EndpointValueMap,
		change: Vec<AllocatedValue>,
	) -> (PartiallySignedTransaction, InmemConsignment<TransferConsignment>);
}

impl RgbUtilities for Client {
	fn issue_contract(
		&mut self, amount: u64, outpoint: OutPoint, ticker: AsciiString, name: AsciiString,
		precision: u8,
	) -> ContractId {
		let allocations = vec![OutpointValue::from_str(&format!("{amount}@{outpoint}"))
			.expect("allocation structure should be correct")];

		let rgb_network =
			Chain::Regtest(BlockHash::from_slice(GENESIS_HASH_REGTEST).expect("valid bloch hash"));

		let asset = Contract::create_rgb20(
			rgb_network,
			ticker,
			name,
			precision,
			allocations,
			BTreeMap::new(),
			CloseMethod::OpretFirst,
			None,
			None,
		);

		let _rgb_asset =
			Rgb20Asset::try_from(&asset).expect("create_rgb20 does not match RGB20 schema");
		self.register_contract(asset.clone(), true, |_| ()).expect("valid contract");

		asset.contract_id()
	}

	fn send_rgb_internal(
		&mut self, contract_id: ContractId, psbt: &mut Psbt,
		input_outpoints_bt: BTreeSet<OutPoint>, beneficiaries: EndpointValueMap,
		change: Vec<AllocatedValue>,
	) -> (PartiallySignedTransaction, InmemConsignment<TransferConsignment>) {
		let contract =
			self.contract(contract_id, vec![], |_| {}).expect("successful contract call");
		psbt.set_rgb_contract(contract.clone()).expect("valid contract");

		let revealed_seal = change.into_iter().map(|v| (v.into_revealed_seal(), v.value)).collect();

		let transfer = self
			.consign(contract_id, vec![], input_outpoints_bt.clone(), |_| ())
			.expect("valid consign call");

		let rgb_asset =
			Rgb20Asset::try_from(&transfer).expect("to have provided a valid consignment");
		let transition = rgb_asset
			.transfer(input_outpoints_bt.clone(), beneficiaries.clone(), revealed_seal)
			.expect("transfer should succeed");

		psbt.push_rgb_transition(transition.clone()).expect("valid transition");

		let node_id = transition.node_id();

		for input in &mut psbt.inputs {
			if input_outpoints_bt.contains(&input.previous_outpoint) {
				input.set_rgb_consumer(contract_id, node_id).expect("set rgb consumer");
			}
		}

		let _count = psbt.rgb_bundle_to_lnpbp4().expect("bundle ok");
		psbt.outputs
			.last_mut()
			.expect("PSBT should have outputs")
			.set_opret_host()
			.expect("given output should be valid");

		let endseals = beneficiaries.into_iter().map(|b| b.0).collect();

		let transfers = vec![(transfer, endseals)];

		let transfer_consignment = self
			.finalize_transfers(transfers, psbt.clone(), |_| ())
			.expect("finalize should succeed");

		let psbt = transfer_consignment.psbt;
		let consignment = transfer_consignment.consignments[0].clone();
		let psbt_serialized =
			&Vec::<u8>::from_hex(&psbt.to_string()).expect("provided psbt should be valid");
		let psbt = deserialize(psbt_serialized).expect("valid psbt");

		(psbt, consignment)
	}

	fn send_rgb(
		&mut self, contract_id: ContractId, psbt: PartiallySignedTransaction,
		input_outpoints_bt: BTreeSet<OutPoint>, beneficiaries: EndpointValueMap,
		change: Vec<AllocatedValue>,
	) -> (PartiallySignedTransaction, InmemConsignment<TransferConsignment>) {
		let mut psbt =
			<Psbt as BitcoinDeserialize>::deserialize(&serialize(&psbt)).expect("valid bdk psbt");

		self.send_rgb_internal(contract_id, &mut psbt, input_outpoints_bt, beneficiaries, change)
	}
}
