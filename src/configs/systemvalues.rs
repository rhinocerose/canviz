#![allow(dead_code)]

use chrono::prelude::*;
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use regex::Regex;
use std::collections::HashMap;
use canparse::pgn::{ParseMessage, PgnLibrary};
use anyhow::{Context, Result};
use socketcan::CANFrame;

#[derive(Debug, Deserialize, FromPrimitive, Copy, Serialize, Clone)]
enum States {
    Standby,
    Charge,
    Discharge,
    EOD,
    Service,
    PreStandby,
    Error,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct NodeValue<'a> {
    identifier: &'a str,
    display_name: &'a str,
    value: f32,
    state: States,
    subsystem: &'a str,
    unit: &'a str,
    frame_id: u32,
    last_updated: DateTime<Local>,
    frames_since_update: i32,
}

impl<'a> NodeValue<'a> {
    fn new(
        identifier: &'a str,
        display_name: &'a str,
        frame_id: u32,
        subsystem: &'a str,
    ) -> NodeValue<'a> {
        NodeValue {
            identifier,
            display_name,
            value: 0.0,
            state: States::Standby,
            subsystem,
            unit: "Stuff",
            frame_id,
            last_updated: Local::now(),
            frames_since_update: -1,
        }
    }

    fn update_value(&mut self, value: f32) {
        self.value = value;
        self.frames_since_update = 0;
        self.last_updated = Local::now();
    }

    fn update_state(&mut self) {
        match FromPrimitive::from_f32(self.value) {
            Some(States::Standby) => self.state = States::Standby,
            Some(States::Charge) => self.state = States::Charge,
            Some(States::Discharge) => self.state = States::Discharge,
            Some(States::EOD) => self.state = States::EOD,
            Some(States::Service) => self.state = States::Service,
            Some(States::PreStandby) => self.state = States::PreStandby,
            Some(States::Error) => self.state = States::Error,
            None => println!("couldn't convert"),
        }
    }

    fn get_identifier(&self) -> &'a str {
        self.identifier
    }

    fn not_updated(&mut self) {
        self.frames_since_update += 1;
    }

    fn get_frame_id(&self) -> u32 {
        self.frame_id
    }

    fn get_subsystem(&self) -> &'a str {
        self.subsystem
    }

    fn print_info(&self) {
        println!(
            "{:<25} {:.2}    Updated: {}",
            self.display_name, self.value, self.frames_since_update
        );
    }
}

#[derive(Debug, Clone)]
pub struct Overview<'a> {
    pub hash_map: HashMap<&'a str, NodeValue<'a>>,
}

impl<'a> Overview<'a> {
    pub fn new() -> Overview<'a> {
        Overview {
            hash_map: HashMap::new(),
        }
    }

    fn join(&mut self, values: NodeValue<'a>) {
        self.hash_map.insert(values.identifier, values);
    }

    fn add_node(
        &mut self,
        identifier: &'a str,
        display_name: &'a str,
        frame_id: u32,
        subsystem: &'a str,
    ) {
        let temp = NodeValue::new(identifier, display_name, frame_id, subsystem);
        self.join(temp.clone());
    }

    pub fn add_nodes(&mut self, file_as_string: &'a str) {
        lazy_static! {
            static ref REG: Regex =
                Regex::new(r"\nidentifier: (.+)\n  display_name: (.+)\n  frame_id: (\d+)\n  subsystem: (.+)").unwrap();
        }

        for cap in REG.captures_iter(file_as_string) {
            let identifier = cap.get(1).map_or("", |m| m.as_str());
            let display_name = cap.get(2).map_or("", |m| m.as_str());
            let frame_id: u32 = cap.get(3).map_or(0, |m| m.as_str().parse::<u32>().unwrap());
            let subsystem = cap.get(4).map_or("", |m| m.as_str());
            self.add_node(identifier, display_name, frame_id, subsystem);
        }
        self.print_info();
    }

    fn update_entry(&mut self, identifier: &'a str, new_entry: f32) {
        self.hash_map.get_mut(identifier).unwrap().update_value(new_entry);
    }

    fn increment(&mut self) {
        for (_, val) in self.hash_map.iter_mut() {
            val.not_updated();
        }
    }

    fn match_frame(&self, frame_id: u32) -> Vec<&'a str> {
        let mut temp: Vec<&str> = Vec::new();
        for (_, val) in self.hash_map.iter() {
            if frame_id == val.get_frame_id() {
                temp.push(val.get_identifier());
            }
        }
        temp
    }

    fn match_state(&mut self) {
        for (_, val) in self.hash_map.iter_mut() {
            if "state" == val.get_subsystem() {
                val.update_state();
                println!("State: {:?}", val.state);
            }
        }
    }

    fn print_info(&self) {
        for (_, val) in self.hash_map.iter() {
            val.print_info();
        }
    }

    fn read_frame(
        &self,
        target: &str,
        library: &PgnLibrary,
        frame: &CANFrame,
    ) -> Result<f32, Box<dyn std::error::Error>> {
        let parsed = library
            .get_spn(target)
            .unwrap()
            .parse_message(frame.data())
            .with_context(|| format!("could not find target string `{}`", target))?;
        Ok(parsed)
    }

    pub fn parse_frame(
        &mut self,
        library: &PgnLibrary,
        frame: &CANFrame
    ) -> Result<(), Box<dyn std::error::Error>> {
        for iterator in self.match_frame(frame.id()) {
            self.update_entry(iterator, self.read_frame(iterator, &library, frame).unwrap());
        }
        self.match_state();
        self.print_info();
        self.increment();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_struct() -> NodeValue<'static> {
        NodeValue::new(
            "temperature_diode",
            "Diode Temperature".to_string(),
            406768872,
        )
    }

    fn make_map() -> Overview<'static> {
        let mut map = Overview::new();
        map.join(make_struct());
        map
    }

    #[test]
    fn frames_since_updates_functioning() {
        let mut temp = make_struct();
        temp.not_updated();
        assert_eq!(temp.frames_since_update, 0);
    }

    #[test]
    fn value_updating_functioning() {
        let mut temp = make_struct();
        temp.update_value(48.0);
        assert_eq!(temp.value, 48.0);
    }

    #[test]
    fn frame_id_return_functioning() {
        let temp = make_struct();
        assert_eq!(temp.frame_id, temp.get_frame_id());
    }
}
