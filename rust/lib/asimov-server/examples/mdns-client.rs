// This is free and unencumbered software released into the public domain.

use asimov_server::mdns::{self, ServiceEvent};

pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    let receiver = mdns::browse()?;

    for event in receiver {
        match event {
            ServiceEvent::SearchStarted(ty) => {
                println!("Search started for service of type {}", ty);
            },
            ServiceEvent::ServiceFound(ty, name) => {
                println!("Found service of type {} with name {}", ty, name);
            },
            ServiceEvent::ServiceResolved(info) => {
                println!("Resolved service: {:?}", info);
            },
            ServiceEvent::ServiceRemoved(ty, name) => {
                println!("Service of type {} with name {} removed", ty, name);
            },
            ServiceEvent::SearchStopped(ty) => {
                println!("Search stopped for service of type {}", ty);
            },
            event => {
                println!("Other service event: {:?}", event);
            },
        }
    }

    Ok(())
}
