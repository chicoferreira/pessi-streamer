// use std::net::SocketAddr;
// use std::time::{Duration, Instant};
// use log::info;
// use common::packet::NodePacket;
// use crate::State;
//
// async fn ping_nodes(state: &State) -> Result<()> {
//     let mut nodes = state.nodes.lock().await;
//     let mut pending_pings = state.pending_pings.lock().await;
//
//     for node in nodes.values_mut() {
//         node.metrics.last_ping_seq += 1;
//         let sequence_number = node.metrics.last_ping_seq;
//         let addr = node.addr;
//         let start_time = Instant::now();
//
//         // Send ping packet
//         let packet = NodePacket::ClientPing { sequence_number };
//         state.socket.send_to(&packet, &addr).await?;
//
//         // Store pending ping
//         pending_pings.insert(sequence_number, (addr, start_time));
//     }
//
//     Ok(())
// }
//
// async fn select_and_switch_node(state: &State) -> Result<()> {
//     let nodes = state.nodes.lock().await;
//     if let Some(best_node_addr) = select_best_node(&nodes) {
//         let mut current_node = state.current_node.lock().await;
//         if Some(best_node_addr) != *current_node {
//             switch_node(state, best_node_addr).await?;
//         }
//     }
//     Ok(())
// }
//
// fn select_best_node(nodes: &HashMap<SocketAddr, Node>) -> Option<SocketAddr> {
//     nodes
//         .iter()
//         .filter_map(|(addr, node)| node.metrics.rtt.map(|rtt| (*addr, rtt)))
//         .min_by_key(|&(_addr, rtt)| rtt)
//         .map(|(addr, _)| addr)
// }
//
// async fn switch_node(state: &State, new_node_addr: SocketAddr) -> Result<()> {
//     let mut current_node = state.current_node.lock().await;
//     if let Some(old_node_addr) = *current_node {
//         // Send StopVideo to old node
//         let packet = NodePacket::StopVideo(0); // Assuming stream_id is 0
//         state.socket.send_to(&packet, &old_node_addr).await?;
//     }
//
//     // Send RequestVideo to new node
//     let packet = NodePacket::RequestVideo(0); // Assuming stream_id is 0
//     state.socket.send_to(&packet, &new_node_addr).await?;
//
//     info!("Switched to node {}", new_node_addr);
//     *current_node = Some(new_node_addr);
//
//     // Reset last_received_time
//     let mut last_received_time = state.last_received_time.lock().await;
//     *last_received_time = Instant::now();
//
//     Ok(())
// }
//
// async fn monitor_node_responsiveness(state: &State) -> Result<()> {
//     let last_received_time = state.last_received_time.lock().await;
//     if Instant::now() - *last_received_time > Duration::from_secs(10) {
//         // No video packets received for 10 seconds
//         // Switch to another node
//         info!("No video packets received for 10 seconds, switching node");
//         select_and_switch_node(state).await?;
//     }
//     Ok(())
// }
