use std::marker::PhantomData;
use std::net::Ipv4Addr;

use if_addrs::{get_if_addrs, IfAddr, Interface};

use crate::colors::Color;
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

use super::Module;

/// Displays the primary non-loopback IPv4 address without opening a socket or
/// sending anything over the network.
pub struct LocalIp<S: LocalIpScheme> {
    scheme: PhantomData<S>,
}

pub trait LocalIpScheme: DefaultColors {
    fn local_ip_fg() -> Color {
        Self::default_fg()
    }

    fn local_ip_bg() -> Color {
        Self::default_bg()
    }
}

impl<S: LocalIpScheme> Default for LocalIp<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: LocalIpScheme> LocalIp<S> {
    pub fn new() -> LocalIp<S> {
        LocalIp {
            scheme: PhantomData,
        }
    }
}

impl<S: LocalIpScheme> Module for LocalIp<S> {
    fn append_segments(&mut self, powerline: &mut Powerline) {
        if let Some(ip) = current_local_ipv4() {
            powerline.add_segment(ip, Style::simple(S::local_ip_fg(), S::local_ip_bg()));
        }
    }
}

/// Return the first useful IPv4 address reported by the host's interfaces.
///
/// Interface enumeration is deliberately used instead of the common UDP
/// "connect to a public address" trick: rendering the prompt must not make a
/// network call, and this also keeps the module useful on an offline machine.
fn current_local_ipv4() -> Option<Ipv4Addr> {
    let interfaces = get_if_addrs().ok()?;
    select_primary_ipv4(&interfaces)
}

fn select_primary_ipv4(interfaces: &[Interface]) -> Option<Ipv4Addr> {
    interfaces
        .iter()
        .enumerate()
        .filter_map(|(order, interface)| {
            let IfAddr::V4(address) = &interface.addr else {
                return None;
            };

            let ip = address.ip;
            is_useful_ipv4(ip).then_some(Ipv4Candidate {
                ip,
                operational: interface.is_oper_up(),
                point_to_point: interface.is_p2p(),
                order,
            })
        })
        .min_by_key(Ipv4Candidate::sort_key)
        .map(|candidate| candidate.ip)
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Ipv4Candidate {
    ip: Ipv4Addr,
    operational: bool,
    point_to_point: bool,
    order: usize,
}

impl Ipv4Candidate {
    /// Prefer an interface reported up, then a regular LAN interface over a
    /// point-to-point tunnel. Keep the OS's order as the final tie-breaker,
    /// which is generally its primary-interface order.
    fn sort_key(&self) -> (bool, bool, usize) {
        (!self.operational, self.point_to_point, self.order)
    }
}

fn is_useful_ipv4(ip: Ipv4Addr) -> bool {
    !ip.is_unspecified() && !ip.is_loopback() && !ip.is_link_local() && !ip.is_multicast()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interface(ip: Ipv4Addr, operational: bool, point_to_point: bool) -> Interface {
        Interface {
            name: "test".into(),
            addr: IfAddr::V4(if_addrs::Ifv4Addr {
                ip,
                netmask: Ipv4Addr::new(255, 255, 255, 0),
                prefixlen: 24,
                broadcast: None,
            }),
            index: None,
            oper_status: if operational {
                if_addrs::IfOperStatus::Up
            } else {
                if_addrs::IfOperStatus::Down
            },
            is_p2p: point_to_point,
            #[cfg(windows)]
            adapter_name: String::new(),
        }
    }

    #[test]
    fn ignores_non_routable_ipv4_addresses() {
        for ip in [
            Ipv4Addr::UNSPECIFIED,
            Ipv4Addr::new(127, 0, 0, 1),
            Ipv4Addr::new(169, 254, 1, 2),
            Ipv4Addr::new(224, 0, 0, 1),
        ] {
            assert!(!is_useful_ipv4(ip), "{ip} should not be displayed");
        }

        assert!(is_useful_ipv4(Ipv4Addr::new(192, 168, 1, 20)));
    }

    #[test]
    fn prefers_up_non_tunnel_interfaces() {
        let interfaces = [
            interface(Ipv4Addr::new(10, 0, 0, 8), false, false),
            interface(Ipv4Addr::new(10, 8, 0, 2), true, true),
            interface(Ipv4Addr::new(192, 168, 1, 20), true, false),
        ];

        assert_eq!(
            select_primary_ipv4(&interfaces),
            Some(Ipv4Addr::new(192, 168, 1, 20))
        );
    }

    #[test]
    fn ignores_ipv6_and_unusable_ipv4_addresses() {
        let interfaces = [
            interface(Ipv4Addr::UNSPECIFIED, true, false),
            interface(Ipv4Addr::new(127, 0, 0, 1), true, false),
            interface(Ipv4Addr::new(169, 254, 1, 2), true, false),
            Interface {
                name: "v6".into(),
                addr: IfAddr::V6(if_addrs::Ifv6Addr {
                    ip: "2001:db8::1".parse().unwrap(),
                    netmask: "ffff:ffff:ffff:ffff::".parse().unwrap(),
                    prefixlen: 64,
                    broadcast: None,
                }),
                index: None,
                oper_status: if_addrs::IfOperStatus::Up,
                is_p2p: false,
                #[cfg(windows)]
                adapter_name: String::new(),
            },
        ];

        assert_eq!(select_primary_ipv4(&interfaces), None);
    }
}
