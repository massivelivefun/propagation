use cpal::traits::HostTrait;

fn main() {
    let available_hosts = cpal::available_hosts();
    println!("Available hosts:");
    for host_id in available_hosts {
        println!("  {:?}", host_id);
    }
    
    let default_host = cpal::default_host();
    println!("Default host: {:?}", default_host.id());
}
