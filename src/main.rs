mod configs;

#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate lazy_static;

use ansi_term::Color::{Cyan, Green, Purple};
use anyhow::Result;
use can_dbc::ByteOrder;
use can_dbc::Signal;
use can_dbc::MultiplexIndicator;
use configs::Overview;
use futures::prelude::*;
use socketcan::CANFrame;
use std::collections::HashMap;
use std::convert::TryInto;
use std::path::PathBuf;
use structopt::StructOpt;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
// use tokio_socketcan;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "canviz",
    about = "Advanced parsing of CAN bus information"
)]
struct Opt {
    /// DBC file path, if not passed frame signals are not decoded
    #[structopt(short = "i", long = "input", parse(from_os_str))]
    input: Option<PathBuf>,

    /// Set can interface
    #[structopt(help = "socketcan CAN interface e.g. vcan0")]
    can_interface: String,

    /// Print raw frame information
    #[structopt(short = "r")]
    raw_data: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let opt = Opt::from_args();
    let raw = opt.raw_data;
    let mut system_values = Overview::new();

    let mut socket_rx = tokio_socketcan::CANSocket::open(&opt.can_interface).unwrap();

    // Read DBC and turn it into a hashmap for lookup
    let signal_lookup = if let Some(dbc_input) = opt.input.as_ref() {
        let mut f = File::open(dbc_input).await?;
        let file_parse: String = std::fs::read_to_string(dbc_input).expect("Invalid config file");
        system_values.add_nodes(&file_parse);
        let mut buffer = Vec::new();
        f.read_to_end(&mut buffer).await?;
        let dbc = can_dbc::DBC::from_slice(&buffer).expect("Failed to parse DBC");

        let mut signal_lookup = HashMap::new();

        for msg in dbc.messages() {
            signal_lookup.insert(
                msg.message_id().0 & !socketcan::EFF_FLAG,
                (msg.message_name().clone(), msg.signals().clone(), "memo"),
            );
        }
        Some(signal_lookup)

    } else {
        None
    };

    while let Some(socket_result) = socket_rx.next().await {
        match socket_result {
            Ok(frame) => {
                if let Some(signal_lookup) = signal_lookup.as_ref() {
                    if (((frame.id() << 16) >> 16) == 0xCCE8) || frame.id() == 0x1831f4e8 {
                        print_dbc_signals(signal_lookup, &frame, raw);
                    }
                }
            }
            Err(err) => {
                eprintln!("IO error: {}", err);
            }
        }
    }
    Ok(())
}

// Given a CAN Frame, lookup the can signals and print the signal values
fn print_dbc_signals(signal_lookup: &HashMap<u32, (String, Vec<Signal>, &str)>, frame: &CANFrame, raw_data: bool) {
    let id = frame.id() & !socketcan::EFF_FLAG;
    let (message_name, signals, _comment) = signal_lookup.get(&id).expect("Unknown message id");
    let message_name_s = format!("{:<30}", message_name);
    println!("\n{} Frame ID: {:08X}", Purple.paint(message_name_s), frame.id());

    for signal in signals.iter() {
        let frame_data: [u8; 8] = frame
            .data()
            .try_into()
            .expect("slice with incorrect length");

        let signal_value: u64 = if *signal.byte_order() == ByteOrder::LittleEndian {
            u64::from_le_bytes(frame_data)
        } else {
            u64::from_be_bytes(frame_data)
        };

        // Calculate signal value
        let bit_mask: u64 = 2u64.pow(*signal.signal_size() as u32) - 1;
        let signal_value = ((signal_value >> signal.start_bit()) & bit_mask) as f32
            * *signal.factor() as f32
            + *signal.offset() as f32;

        let signal_value_s = format!("{:6.2}", signal_value);
        let signal_name_s = format!("{:<30}", signal.name());

        let muxed: bool = *signal.multiplexer_indicator() == MultiplexIndicator::MultiplexedSignal(frame_data[0] as u64);
        let plain: bool = *signal.multiplexer_indicator() == MultiplexIndicator::Plain;

        if plain || (raw_data && muxed) {
            println!(
                "{} → {} {}",
                Green.paint(signal_name_s),
                Cyan.paint(signal_value_s.clone()),
                Cyan.paint(signal.unit()),
            );
        };
    }
}
