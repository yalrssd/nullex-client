use crate::core::factory::get_runtime_config;
use crate::core::mihomo_api::{mihomo_proxies, mihomo_proxy_providers};
use crate::state::AppState;
use crate::utils::dirs::{mihomo_profile_work_dir, mihomo_work_dir};
use crate::utils::yaml::parse_yaml_to_json;
use chrono::Utc;
use futures_util::future::join_all;
use network_interface::{Addr, NetworkInterface, NetworkInterfaceConfig};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::{TcpSocket, UdpSocket};
use tokio::sync::{Mutex, Semaphore};

const MAX_HISTORY_PER_PROXY: usize = 10;

#[derive(Debug, Clone)]
pub struct ProxyEndpoint {
    pub server: String,
    pub port: u16,
    pub proxy_type: String,
}

#[derive(Debug, Clone)]
pub struct PhysicalOutbound {
    pub if_index: u32,
    pub local_v4: Option<Ipv4Addr>,
    pub local_v6: Option<Ipv6Addr>,
    pub tun_active: bool,
}

fn is_virtual_or_tun_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("mihomo")
        || lower.contains("wintun")
        || lower.contains("utun")
        || lower.contains("loopback")
        || lower.contains("tailscale")
        || lower.contains("wireguard")
        || lower.contains("zerotier")
        || lower.contains("clash")
        || lower.starts_with("tun")
        || lower.starts_with("tap")
}

pub fn is_fake_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == 198 && (o[1] == 18 || o[1] == 19)
        }
        IpAddr::V6(_) => false,
    }
}

fn is_usable_v4(v4: Ipv4Addr) -> bool {
    !v4.is_loopback()
        && !v4.is_unspecified()
        && !v4.is_link_local()
        && !is_fake_ip(&IpAddr::V4(v4))
}

fn is_usable_v6(v6: Ipv6Addr) -> bool {
    let seg0 = v6.segments()[0];
    !v6.is_loopback() && !v6.is_unspecified() && (seg0 & 0xffc0) != 0xfe80
}

#[cfg(windows)]
fn best_default_v4_if_index(allowed: &HashMap<u32, (Option<Ipv4Addr>, Option<Ipv6Addr>)>) -> Option<u32> {
    use windows_sys::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
    use windows_sys::Win32::NetworkManagement::IpHelper::{GetIpForwardTable, MIB_IPFORWARDTABLE};

    unsafe {
        let mut size: u32 = 0;
        let rc = GetIpForwardTable(std::ptr::null_mut(), &mut size, 0);
        if rc != ERROR_INSUFFICIENT_BUFFER && rc != 0 {
            return None;
        }
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        let table_ptr = buf.as_mut_ptr() as *mut MIB_IPFORWARDTABLE;
        if GetIpForwardTable(table_ptr, &mut size, 0) != 0 {
            return None;
        }
        let count = (*table_ptr).dwNumEntries as usize;
        let rows = std::slice::from_raw_parts((*table_ptr).table.as_ptr(), count);

        let mut best: Option<(u32, u32)> = None;
        for row in rows {
            if row.dwForwardDest != 0 || row.dwForwardMask != 0 {
                continue;
            }
            if !allowed
                .get(&row.dwForwardIfIndex)
                .and_then(|(v4, _)| *v4)
                .is_some()
            {
                continue;
            }
            match best {
                Some((_, best_metric)) if row.dwForwardMetric1 >= best_metric => {}
                _ => best = Some((row.dwForwardIfIndex, row.dwForwardMetric1)),
            }
        }
        best.map(|(idx, _)| idx)
    }
}

#[cfg(not(windows))]
fn best_default_v4_if_index(_allowed: &HashMap<u32, (Option<Ipv4Addr>, Option<Ipv6Addr>)>) -> Option<u32> {
    None
}

pub fn discover_physical_outbound() -> Option<PhysicalOutbound> {
    let ifaces = NetworkInterface::show().ok()?;
    let mut tun_active = false;
    let mut physical_addrs: HashMap<u32, (Option<Ipv4Addr>, Option<Ipv6Addr>)> = HashMap::new();

    for iface in &ifaces {
        if is_virtual_or_tun_name(&iface.name) {
            let lower = iface.name.to_ascii_lowercase();
            if lower.contains("mihomo") || lower.contains("wintun") || lower.starts_with("tun") {
                tun_active = true;
            }
            continue;
        }
        let mut v4 = None;
        let mut v6 = None;
        for addr in &iface.addr {
            match addr {
                Addr::V4(a) if v4.is_none() && is_usable_v4(a.ip) => v4 = Some(a.ip),
                Addr::V6(a) if v6.is_none() && is_usable_v6(a.ip) => v6 = Some(a.ip),
                _ => {}
            }
        }
        if v4.is_some() || v6.is_some() {
            physical_addrs.insert(iface.index, (v4, v6));
        }
    }

    if let Some(best_idx) = best_default_v4_if_index(&physical_addrs) {
        if let Some(&(local_v4, local_v6)) = physical_addrs.get(&best_idx) {
            return Some(PhysicalOutbound {
                if_index: best_idx,
                local_v4,
                local_v6,
                tun_active,
            });
        }
    }

    physical_addrs
        .into_iter()
        .find(|(_, (v4, _))| v4.is_some())
        .map(|(if_index, (local_v4, local_v6))| PhysicalOutbound {
            if_index,
            local_v4,
            local_v6,
            tun_active,
        })
}

#[cfg(windows)]
fn bind_socket_to_if<S: std::os::windows::io::AsRawSocket>(
    sock: &S,
    if_index: u32,
    is_ipv6: bool,
) {
    use windows_sys::Win32::Networking::WinSock::{
        setsockopt, IPPROTO_IP, IPPROTO_IPV6, IPV6_UNICAST_IF, IP_UNICAST_IF, SOCKET,
    };

    let raw = sock.as_raw_socket() as SOCKET;
    unsafe {
        if is_ipv6 {
            let idx = if_index;
            let _ = setsockopt(
                raw,
                IPPROTO_IPV6 as i32,
                IPV6_UNICAST_IF as i32,
                (&idx as *const u32) as *const u8,
                std::mem::size_of::<u32>() as i32,
            );
        } else {
            let idx_be = if_index.to_be();
            let _ = setsockopt(
                raw,
                IPPROTO_IP as i32,
                IP_UNICAST_IF as i32,
                (&idx_be as *const u32) as *const u8,
                std::mem::size_of::<u32>() as i32,
            );
        }
    }
}

#[cfg(not(windows))]
fn bind_socket_to_if<S>(_sock: &S, _if_index: u32, _is_ipv6: bool) {}

pub fn build_dns_a_query(domain: &str, tx_id: u16) -> Option<Vec<u8>> {
    let trimmed = domain.trim().trim_end_matches('.');
    if trimmed.is_empty() || trimmed.len() > 253 {
        return None;
    }
    let mut pkt = Vec::with_capacity(trimmed.len() + 20);
    pkt.extend_from_slice(&tx_id.to_be_bytes());
    pkt.extend_from_slice(&[0x01, 0x00]); // standard query, recursion desired
    pkt.extend_from_slice(&1u16.to_be_bytes()); // QDCOUNT = 1
    pkt.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // ANCOUNT, NSCOUNT, ARCOUNT

    for label in trimmed.split('.') {
        let bytes = label.as_bytes();
        if bytes.is_empty() || bytes.len() > 63 {
            return None;
        }
        pkt.push(bytes.len() as u8);
        pkt.extend_from_slice(bytes);
    }
    pkt.push(0); // root label
    pkt.extend_from_slice(&1u16.to_be_bytes()); // QTYPE = A
    pkt.extend_from_slice(&1u16.to_be_bytes()); // QCLASS = IN
    Some(pkt)
}

fn skip_dns_name(buf: &[u8], mut offset: usize) -> Option<usize> {
    let mut jumps = 0;
    loop {
        if offset >= buf.len() || jumps > 16 {
            return None;
        }
        let len = buf[offset];
        if (len & 0xC0) == 0xC0 {
            if offset + 1 >= buf.len() {
                return None;
            }
            return Some(offset + 2);
        } else if len == 0 {
            return Some(offset + 1);
        } else {
            offset = offset.checked_add(1 + len as usize)?;
            jumps += 1;
        }
    }
}

pub fn parse_dns_a_response(buf: &[u8], expected_id: u16) -> Vec<IpAddr> {
    if buf.len() < 12 {
        return Vec::new();
    }
    let tx_id = u16::from_be_bytes([buf[0], buf[1]]);
    if tx_id != expected_id {
        return Vec::new();
    }
    let qdcount = u16::from_be_bytes([buf[4], buf[5]]) as usize;
    let ancount = u16::from_be_bytes([buf[6], buf[7]]) as usize;

    let mut offset = 12;
    for _ in 0..qdcount {
        let Some(next) = skip_dns_name(buf, offset) else {
            return Vec::new();
        };
        offset = next + 4;
        if offset > buf.len() {
            return Vec::new();
        }
    }

    let mut ips = Vec::new();
    for _ in 0..ancount {
        let Some(next) = skip_dns_name(buf, offset) else {
            break;
        };
        offset = next;
        if offset + 10 > buf.len() {
            break;
        }
        let rtype = u16::from_be_bytes([buf[offset], buf[offset + 1]]);
        let rclass = u16::from_be_bytes([buf[offset + 2], buf[offset + 3]]);
        let rdlen = u16::from_be_bytes([buf[offset + 8], buf[offset + 9]]) as usize;
        offset += 10;
        if offset + rdlen > buf.len() {
            break;
        }
        if rtype == 1 && rclass == 1 && rdlen == 4 {
            let ip = IpAddr::V4(Ipv4Addr::new(
                buf[offset],
                buf[offset + 1],
                buf[offset + 2],
                buf[offset + 3],
            ));
            if !is_fake_ip(&ip) && !ip.is_unspecified() {
                ips.push(ip);
            }
        }
        offset += rdlen;
    }
    ips
}

async fn resolve_via_direct_udp_dns(
    domain: &str,
    outbound: Option<&PhysicalOutbound>,
) -> Vec<IpAddr> {
    let tx_id: u16 = rand::random();
    let Some(query) = build_dns_a_query(domain, tx_id) else {
        return Vec::new();
    };
    let servers = [
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), 53),
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 53),
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(223, 5, 5, 5)), 53),
    ];

    let bind_addr = match outbound.and_then(|o| o.local_v4) {
        Some(v4) => SocketAddr::new(IpAddr::V4(v4), 0),
        None => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
    };

    let sock = match UdpSocket::bind(bind_addr).await {
        Ok(s) => s,
        Err(_) => match UdpSocket::bind("0.0.0.0:0").await {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        },
    };

    if let Some(out) = outbound {
        bind_socket_to_if(&sock, out.if_index, false);
    }

    for dns_srv in servers {
        if sock.send_to(&query, dns_srv).await.is_err() {
            continue;
        }
        let mut buf = [0u8; 512];
        if let Ok(Ok((len, _))) =
            tokio::time::timeout(Duration::from_millis(1200), sock.recv_from(&mut buf)).await
        {
            let ips = parse_dns_a_response(&buf[..len], tx_id);
            if !ips.is_empty() {
                return ips;
            }
        }
    }
    Vec::new()
}

fn parse_ip_literal(host: &str) -> Option<IpAddr> {
    let trimmed = host.trim();
    let unbracketed = trimmed
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(trimmed);
    unbracketed.parse::<IpAddr>().ok()
}

fn collect_hosts_map(runtime_cfg: &Value) -> HashMap<String, Vec<IpAddr>> {
    let mut map = HashMap::new();
    let Some(hosts_obj) = runtime_cfg.get("hosts").and_then(Value::as_object) else {
        return map;
    };
    for (domain, val) in hosts_obj {
        let mut ips = Vec::new();
        match val {
            Value::String(s) => {
                if let Some(ip) = parse_ip_literal(s) {
                    if !is_fake_ip(&ip) {
                        ips.push(ip);
                    }
                }
            }
            Value::Array(arr) => {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        if let Some(ip) = parse_ip_literal(s) {
                            if !is_fake_ip(&ip) {
                                ips.push(ip);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        if !ips.is_empty() {
            map.insert(domain.to_ascii_lowercase(), ips);
        }
    }
    map
}

async fn resolve_server_ips(
    host: &str,
    port: u16,
    hosts_map: &HashMap<String, Vec<IpAddr>>,
    outbound: Option<&PhysicalOutbound>,
    dns_cache: &Arc<Mutex<HashMap<String, Vec<IpAddr>>>>,
) -> Vec<IpAddr> {
    if let Some(ip) = parse_ip_literal(host) {
        return vec![ip];
    }

    let key = host.trim().to_ascii_lowercase();
    if let Some(ips) = hosts_map.get(&key) {
        return ips.clone();
    }

    if let Some(cached) = dns_cache.lock().await.get(&key).cloned() {
        return cached;
    }

    let tun_active = outbound.map(|o| o.tun_active).unwrap_or(false);
    let mut resolved = Vec::new();

    if !tun_active {
        if let Ok(Ok(addrs)) =
            tokio::time::timeout(Duration::from_millis(2000), tokio::net::lookup_host((host, port)))
                .await
        {
            for addr in addrs {
                let ip = addr.ip();
                if !is_fake_ip(&ip) && !ip.is_unspecified() && !resolved.contains(&ip) {
                    resolved.push(ip);
                }
            }
        }
    }

    if resolved.is_empty() {
        resolved = resolve_via_direct_udp_dns(host, outbound).await;
    }

    if resolved.is_empty() && tun_active {
        if let Ok(Ok(addrs)) =
            tokio::time::timeout(Duration::from_millis(1500), tokio::net::lookup_host((host, port)))
                .await
        {
            for addr in addrs {
                let ip = addr.ip();
                if !is_fake_ip(&ip) && !ip.is_unspecified() && !resolved.contains(&ip) {
                    resolved.push(ip);
                }
            }
        }
    }

    // Prefer IPv4 first so v4 physical NIC binding succeeds immediately when both v4/v6 exist
    resolved.sort_by_key(|ip| match ip {
        IpAddr::V4(_) => 0,
        IpAddr::V6(_) => 1,
    });

    if !resolved.is_empty() {
        dns_cache.lock().await.insert(key, resolved.clone());
    }
    resolved
}

pub async fn tcp_connect_rtt_ms(
    target: SocketAddr,
    outbound: Option<&PhysicalOutbound>,
    timeout_ms: u64,
) -> Option<u64> {
    let sock = match target {
        SocketAddr::V4(_) => TcpSocket::new_v4().ok()?,
        SocketAddr::V6(_) => TcpSocket::new_v6().ok()?,
    };

    if !target.ip().is_loopback() {
        if let Some(out) = outbound {
            match target {
                SocketAddr::V4(_) => {
                    if let Some(v4) = out.local_v4 {
                        bind_socket_to_if(&sock, out.if_index, false);
                        let _ = sock.bind(SocketAddr::new(IpAddr::V4(v4), 0));
                    }
                }
                SocketAddr::V6(_) => {
                    if let Some(v6) = out.local_v6 {
                        bind_socket_to_if(&sock, out.if_index, true);
                        let _ = sock.bind(SocketAddr::new(IpAddr::V6(v6), 0));
                    }
                }
            }
        }
    }

    let start = Instant::now();
    let res = tokio::time::timeout(Duration::from_millis(timeout_ms), sock.connect(target)).await;
    match res {
        Ok(Ok(stream)) => {
            drop(stream);
            Some(start.elapsed().as_millis().max(1) as u64)
        }
        _ => None,
    }
}

fn parse_port(val: Option<&Value>) -> Option<u16> {
    match val {
        Some(Value::Number(n)) => n.as_u64().and_then(|p| u16::try_from(p).ok()),
        Some(Value::String(s)) => s.trim().parse::<u16>().ok(),
        _ => None,
    }
    .filter(|&p| p > 0)
}

fn extract_proxies_into_map(proxies_val: Option<&Value>, map: &mut HashMap<String, ProxyEndpoint>) {
    let Some(arr) = proxies_val.and_then(Value::as_array) else {
        return;
    };
    for p in arr {
        let Some(name) = p.get("name").and_then(Value::as_str).map(str::trim) else {
            continue;
        };
        let Some(server) = p.get("server").and_then(Value::as_str).map(str::trim) else {
            continue;
        };
        let Some(port) = parse_port(p.get("port")) else {
            continue;
        };
        if name.is_empty() || server.is_empty() {
            continue;
        }
        let proxy_type = p
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        map.entry(name.to_string()).or_insert(ProxyEndpoint {
            server: server.to_string(),
            port,
            proxy_type,
        });
    }
}

async fn resolve_provider_file_paths(
    state: &Arc<AppState>,
    runtime_cfg: &Value,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut work_dirs = vec![mihomo_work_dir()];
    if let Ok(prof_cfg) = crate::config::profile::get_profile_config(state, false).await {
        if let Some(cur) = prof_cfg.get("current").and_then(Value::as_str) {
            work_dirs.push(mihomo_profile_work_dir(Some(cur)));
        }
    }

    if let Some(prov_obj) = runtime_cfg.get("proxy-providers").and_then(Value::as_object) {
        for (name, prov) in prov_obj {
            if let Some(rel) = prov.get("path").and_then(Value::as_str) {
                let p = Path::new(rel);
                if p.is_absolute() {
                    paths.push(p.to_path_buf());
                } else {
                    for wd in &work_dirs {
                        paths.push(wd.join(p));
                    }
                }
            } else {
                for wd in &work_dirs {
                    paths.push(wd.join("proxy_providers").join(format!("{name}.yaml")));
                    paths.push(wd.join("proxy_providers").join(name));
                }
            }
        }
    }
    paths
}

async fn collect_endpoints(
    state: &Arc<AppState>,
    runtime_cfg: &Value,
) -> HashMap<String, ProxyEndpoint> {
    let mut endpoints = HashMap::new();
    extract_proxies_into_map(runtime_cfg.get("proxies"), &mut endpoints);

    // Also include proxy-providers from disk if present
    let provider_paths = resolve_provider_file_paths(state, runtime_cfg).await;
    let mut seen_paths = HashSet::new();
    for path in provider_paths {
        if !seen_paths.insert(path.clone()) || !path.is_file() {
            continue;
        }
        if let Ok(raw) = tokio::fs::read_to_string(&path).await {
            if let Ok(parsed) = parse_yaml_to_json(&raw) {
                extract_proxies_into_map(parsed.get("proxies"), &mut endpoints);
            }
        }
    }

    // Fallback: ask Mihomo /providers/proxies for any inline provider metadata
    if let Ok(prov_root) = mihomo_proxy_providers().await {
        if let Some(prov_map) = prov_root.get("providers").and_then(Value::as_object) {
            for prov in prov_map.values() {
                extract_proxies_into_map(prov.get("proxies"), &mut endpoints);
            }
        }
    }

    endpoints
}

pub fn resolve_leaf_proxy_name<'a>(
    start_name: &'a str,
    proxies_map: &'a Map<String, Value>,
) -> &'a str {
    let mut current = start_name;
    let mut visited = HashSet::new();
    for _ in 0..10 {
        if !visited.insert(current) {
            break;
        }
        let Some(node) = proxies_map.get(current) else {
            break;
        };
        let is_group = node.get("all").map(Value::is_array).unwrap_or(false);
        if !is_group {
            return current;
        }
        let Some(next) = node
            .get("now")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            break;
        };
        current = next;
    }
    current
}

async fn measure_endpoint_delay(
    ep: &ProxyEndpoint,
    hosts_map: &HashMap<String, Vec<IpAddr>>,
    outbound: Option<&PhysicalOutbound>,
    dns_cache: &Arc<Mutex<HashMap<String, Vec<IpAddr>>>>,
    timeout_ms: u64,
) -> u64 {
    let ips = resolve_server_ips(&ep.server, ep.port, hosts_map, outbound, dns_cache).await;
    if ips.is_empty() {
        return 0;
    }
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    for ip in ips.into_iter().take(2) {
        let remaining = deadline
            .saturating_duration_since(Instant::now())
            .as_millis() as u64;
        if remaining == 0 {
            break;
        }
        let target = SocketAddr::new(ip, ep.port);
        if let Some(ms) = tcp_connect_rtt_ms(target, outbound, remaining).await {
            return ms;
        }
    }
    0
}

fn push_history(map: &mut HashMap<String, Vec<Value>>, name: String, delay: u64, time_iso: &str) {
    let entry = json!({
        "time": time_iso,
        "delay": delay
    });
    let list = map.entry(name).or_default();
    list.push(entry);
    if list.len() > MAX_HISTORY_PER_PROXY {
        let excess = list.len() - MAX_HISTORY_PER_PROXY;
        list.drain(0..excess);
    }
}

pub async fn tcp_proxy_delay(
    state: &Arc<AppState>,
    proxy_name: &str,
    timeout_ms: u64,
) -> Result<Value, String> {
    let runtime_cfg = get_runtime_config(state).await.unwrap_or_default();
    let hosts_map = collect_hosts_map(&runtime_cfg);
    let endpoints = collect_endpoints(state, &runtime_cfg).await;
    let mihomo_nodes = mihomo_proxies()
        .await
        .ok()
        .and_then(|v| v.get("proxies").cloned())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default();

    let leaf = resolve_leaf_proxy_name(proxy_name, &mihomo_nodes);
    let outbound = tokio::task::spawn_blocking(discover_physical_outbound)
        .await
        .ok()
        .flatten();
    let dns_cache = Arc::new(Mutex::new(HashMap::new()));

    let delay = if let Some(ep) = endpoints.get(leaf) {
        measure_endpoint_delay(ep, &hosts_map, outbound.as_ref(), &dns_cache, timeout_ms).await
    } else {
        0
    };

    let now_iso = Utc::now().to_rfc3339();
    {
        let mut hist = state.tcp_delay_history.write().await;
        push_history(&mut hist, proxy_name.to_string(), delay, &now_iso);
        if leaf != proxy_name {
            push_history(&mut hist, leaf.to_string(), delay, &now_iso);
        }
    }

    Ok(json!({ "delay": delay }))
}

pub async fn tcp_group_delay(
    state: &Arc<AppState>,
    group_name: &str,
    timeout_ms: u64,
    concurrency: usize,
) -> Result<Value, String> {
    let runtime_cfg = get_runtime_config(state).await.unwrap_or_default();
    let hosts_map = collect_hosts_map(&runtime_cfg);
    let endpoints = Arc::new(collect_endpoints(state, &runtime_cfg).await);
    let mihomo_nodes = mihomo_proxies()
        .await
        .ok()
        .and_then(|v| v.get("proxies").cloned())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default();

    let member_names: Vec<String> = mihomo_nodes
        .get(group_name)
        .and_then(|g| g.get("all"))
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_else(|| {
            runtime_cfg
                .get("proxy-groups")
                .and_then(Value::as_array)
                .and_then(|pgs| {
                    pgs.iter()
                        .find(|pg| pg.get("name").and_then(Value::as_str) == Some(group_name))
                })
                .and_then(|pg| pg.get("proxies"))
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default()
        });

    let outbound = tokio::task::spawn_blocking(discover_physical_outbound)
        .await
        .ok()
        .flatten()
        .map(Arc::new);
    let dns_cache = Arc::new(Mutex::new(HashMap::new()));
    let hosts_map = Arc::new(hosts_map);
    let sem = Arc::new(Semaphore::new(concurrency.clamp(1, 200)));

    let tasks = member_names.into_iter().map(|member| {
        let leaf = resolve_leaf_proxy_name(&member, &mihomo_nodes).to_string();
        let endpoints = Arc::clone(&endpoints);
        let outbound = outbound.clone();
        let dns_cache = Arc::clone(&dns_cache);
        let hosts_map = Arc::clone(&hosts_map);
        let sem = Arc::clone(&sem);

        async move {
            let _permit = sem.acquire_owned().await.ok();
            let delay = if let Some(ep) = endpoints.get(&leaf) {
                measure_endpoint_delay(
                    ep,
                    &hosts_map,
                    outbound.as_deref(),
                    &dns_cache,
                    timeout_ms,
                )
                .await
            } else {
                0
            };
            (member, leaf, delay)
        }
    });

    let results = join_all(tasks).await;
    let now_iso = Utc::now().to_rfc3339();
    let mut out_map = Map::new();

    {
        let mut hist = state.tcp_delay_history.write().await;
        for (member, leaf, delay) in results {
            push_history(&mut hist, member.clone(), delay, &now_iso);
            if leaf != member {
                push_history(&mut hist, leaf, delay, &now_iso);
            }
            if delay > 0 {
                out_map.insert(member, json!(delay));
            }
        }
    }

    Ok(Value::Object(out_map))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn detects_fake_ip_range() {
        assert!(is_fake_ip(&IpAddr::V4(Ipv4Addr::new(198, 18, 0, 1))));
        assert!(is_fake_ip(&IpAddr::V4(Ipv4Addr::new(198, 19, 255, 254))));
        assert!(!is_fake_ip(&IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))));
        assert!(!is_fake_ip(&IpAddr::V4(Ipv4Addr::new(198, 20, 0, 1))));
    }

    #[test]
    fn builds_and_parses_dns_a_packet() {
        let query = build_dns_a_query("vpn.example.com", 0x1234).unwrap();
        assert_eq!(&query[0..2], &[0x12, 0x34]);

        // Craft DNS response reusing header+question with answer pointer 0xC0 0x0C -> 93.184.216.34
        let mut resp = query.clone();
        resp[2] = 0x81;
        resp[3] = 0x80;
        resp[6] = 0x00;
        resp[7] = 0x01; // ANCOUNT = 1
        resp.extend_from_slice(&[0xC0, 0x0C]); // Name ptr
        resp.extend_from_slice(&1u16.to_be_bytes()); // TYPE A
        resp.extend_from_slice(&1u16.to_be_bytes()); // CLASS IN
        resp.extend_from_slice(&60u32.to_be_bytes()); // TTL
        resp.extend_from_slice(&4u16.to_be_bytes()); // RDLENGTH
        resp.extend_from_slice(&[93, 184, 216, 34]);

        let ips = parse_dns_a_response(&resp, 0x1234);
        assert_eq!(ips, vec![IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))]);
        assert!(parse_dns_a_response(&resp, 0x9999).is_empty());
    }

    #[tokio::test]
    async fn measures_loopback_tcp_handshake() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let accept_task = tokio::spawn(async move {
            let _ = listener.accept().await;
        });

        let rtt = tcp_connect_rtt_ms(addr, None, 2000).await;
        let _ = accept_task.await;
        assert!(rtt.is_some());
        assert!(rtt.unwrap() >= 1);
    }
}
