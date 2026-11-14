//! A dragged web image may name a URL, but it must not turn the app into
//! a way for a web page to read services on this Mac or its private network.
use std::net::IpAddr;
#[cfg(any(test, feature = "automation"))]
use std::net::SocketAddr;
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{DefaultConnector, NextTimeout};

#[derive(Debug, Default)]
pub(crate) struct PublicResolver<R = DefaultResolver> {
    inner: R,
    // The scripted app's isolated pasteboard can explicitly allow its own
    // local fixture server. This field and the escape hatch do not ship.
    #[cfg(any(test, feature = "automation"))]
    pub(crate) fixture: Option<SocketAddr>,
}

#[cfg(feature = "automation")]
impl PublicResolver {
    pub(crate) fn allow_fixture(mut self, address: Option<SocketAddr>) -> Self {
        self.fixture = address;
        self
    }
}

impl<R: Resolver> Resolver for PublicResolver<R> {
    fn resolve(&self, uri: &ureq::http::Uri, config: &ureq::config::Config, timeout: NextTimeout) -> Result<ResolvedSocketAddrs, ureq::Error> {
        if !matches!(uri.scheme_str(), Some("http" | "https")) || uri.authority().is_some_and(|a| a.as_str().contains('@')) {
            return Err(blocked());
        }
        // Filter the actual socket addresses given to the TCP connector,
        // rather than doing a check followed by a second DNS lookup. ureq
        // also uses this resolver for each redirected connection.
        let resolved = self.inner.resolve(uri, config, timeout)?;
        let local = interface_networks().map_err(ureq::Error::Io)?;
        let mut allowed = self.empty();
        for address in resolved.iter().copied() {
            let public = public_destination(address.ip(), &local);
            #[cfg(any(test, feature = "automation"))]
            let public = public || self.fixture == Some(address);
            if public { allowed.push(address); }
        }
        if allowed.is_empty() { return Err(blocked()); }
        Ok(allowed)
    }
}

fn blocked() -> ureq::Error {
    ureq::Error::Io(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Browser pictures must use a public internet address"))
}

struct LocalNetwork { address: IpAddr, mask: IpAddr }
impl LocalNetwork {
    fn contains(&self, ip: IpAddr) -> bool {
        match (ip, self.address, self.mask) {
            (IpAddr::V4(ip), IpAddr::V4(own), IpAddr::V4(mask)) => u32::from(ip) & u32::from(mask) == u32::from(own) & u32::from(mask),
            (IpAddr::V6(ip), IpAddr::V6(own), IpAddr::V6(mask)) => u128::from(ip) & u128::from(mask) == u128::from(own) & u128::from(mask),
            _ => false,
        }
    }
}
fn public_destination(ip: IpAddr, local: &[LocalNetwork]) -> bool {
    public_address(ip) && !local.iter().any(|network| network.contains(ip) || nat64_target(ip).is_some_and(|target| network.contains(target)))
}

// Global IPv6 addresses can belong to this Mac or a neighbour on its LAN.
// Refuse each directly attached network as well as special-purpose ranges.
fn interface_networks() -> std::io::Result<Vec<LocalNetwork>> {
    let mut first = std::ptr::null_mut();
    // SAFETY: getifaddrs supplies a linked list and socket addresses valid
    // until freeifaddrs. socket_ip checks the family before every cast.
    unsafe {
        if libc::getifaddrs(&mut first) != 0 { return Err(std::io::Error::last_os_error()); }
        let mut networks = Vec::new();
        let mut item = first;
        while let Some(interface) = item.as_ref() {
            if let Some(address) = socket_ip(interface.ifa_addr) {
                // An interface without a netmask still excludes its own IP.
                let mask = socket_ip(interface.ifa_netmask).filter(|mask| mask.is_ipv4() == address.is_ipv4())
                    .unwrap_or_else(|| match address { IpAddr::V4(_) => IpAddr::V4(std::net::Ipv4Addr::from(u32::MAX)), IpAddr::V6(_) => IpAddr::V6(std::net::Ipv6Addr::from(u128::MAX)) });
                networks.push(LocalNetwork { address, mask });
            }
            item = interface.ifa_next;
        }
        libc::freeifaddrs(first);
        Ok(networks)
    }
}

unsafe fn socket_ip(pointer: *const libc::sockaddr) -> Option<IpAddr> {
    // SAFETY: the caller holds getifaddrs' list; a socket's family identifies
    // its concrete address structure. A null address is allowed in the list.
    unsafe {
        match i32::from(pointer.as_ref()?.sa_family) {
            libc::AF_INET => Some(IpAddr::V4(std::net::Ipv4Addr::from((*pointer.cast::<libc::sockaddr_in>()).sin_addr.s_addr.to_ne_bytes()))),
            libc::AF_INET6 => Some(IpAddr::V6(std::net::Ipv6Addr::from((*pointer.cast::<libc::sockaddr_in6>()).sin6_addr.s6_addr))),
            _ => None,
        }
    }
}

fn nat64_target(ip: IpAddr) -> Option<IpAddr> {
    let IpAddr::V6(ip) = ip else { return None };
    if ip.segments()[..6] != [0x64, 0xff9b, 0, 0, 0, 0] { return None; }
    // RFC 6052 §2.3 puts the IPv4 target in the last 32 bits.
    let bytes = ip.octets();
    Some(IpAddr::V4(std::net::Ipv4Addr::new(bytes[12], bytes[13], bytes[14], bytes[15])))
}

pub(crate) fn agent(resolver: impl Resolver) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        // A proxy could resolve the target again, outside the policy above.
        .proxy(None)
        .max_redirects(5)
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .build();
    ureq::Agent::with_parts(config, DefaultConnector::default(), resolver)
}

// Special-purpose blocks: https://www.iana.org/assignments/iana-ipv4-special-registry/
// and https://www.iana.org/assignments/iana-ipv6-special-registry/.
fn public_address(ip: IpAddr) -> bool {
    // Keep public IPv4 hosts working on IPv6-only networks, while still
    // checking the target carried by the standard NAT64 prefix.
    if let Some(target) = nat64_target(ip) { return public_address(target); }
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(a == 0 || a == 10 || a == 127 || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && (b == 168 || (b == 0 && c == 0) || (b == 0 && c == 2) || (b == 88 && c == 99)))
                || (a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(ip) => {
            let [a, b, ..] = ip.segments();
            // Global unicast only; exclude special-purpose/documentation
            // ranges and 6to4, which can tunnel to an embedded private IPv4.
            // This also excludes mapped IPv4, local-use NAT64, ULA and link-local.
            a & 0xe000 == 0x2000 && a != 0x2002
                && !(a == 0x2001 && (b < 0x0200 || b == 0x0db8))
                && !(a == 0x3fff && b < 0x1000)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_global_address_assigned_to_this_mac_is_not_a_web_destination() {
        let own = "2606:4700:4700::1111".parse().unwrap();
        let network = LocalNetwork { address: own, mask: "ffff:ffff:ffff:ffff::".parse().unwrap() };
        assert!(!public_destination(own, std::slice::from_ref(&network)));
        assert!(!public_destination("2606:4700:4700::1".parse().unwrap(), std::slice::from_ref(&network)));
        assert!(public_destination("2001:4860:4860::8888".parse().unwrap(), &[network]));
        assert!(!public_destination("64:ff9b::808:808".parse().unwrap(), &[LocalNetwork { address: "8.8.8.1".parse().unwrap(), mask: "255.255.255.0".parse().unwrap() }]));
    }

    #[test]
    fn the_interface_list_includes_loopback() {
        let networks = interface_networks().unwrap();
        assert!(networks.iter().any(|network| network.contains("127.0.0.1".parse().unwrap())));
    }

    #[test]
    fn only_public_addresses_are_eligible() {
        for ip in ["0.0.0.0", "10.0.0.1", "100.64.0.1", "127.0.0.1", "169.254.169.254", "172.31.0.1", "192.168.1.1", "192.0.0.1", "192.0.2.1", "192.88.99.1", "198.18.0.1", "198.51.100.1", "203.0.113.1", "224.0.0.1", "255.255.255.255", "::", "::1", "::ffff:127.0.0.1", "64:ff9b::7f00:1", "fc00::1", "fe80::1", "ff02::1", "2001::7f00:1", "2001:db8::1", "2002:7f00:1::1", "3fff::1"] {
            assert!(!public_address(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["1.1.1.1", "8.8.8.8", "93.184.216.34", "2606:4700:4700::1111", "2001:4860:4860::8888", "64:ff9b::808:808", "64:ff9b::101:101"] {
            assert!(public_address(ip.parse().unwrap()), "{ip}");
        }
    }

    #[derive(Debug)]
    struct Answers(Vec<SocketAddr>);
    impl Resolver for Answers {
        fn resolve(&self, _: &ureq::http::Uri, _: &ureq::config::Config, _: NextTimeout) -> Result<ResolvedSocketAddrs, ureq::Error> {
            let mut out = self.empty();
            for address in &self.0 { out.push(*address); }
            Ok(out)
        }
    }

    #[test]
    fn the_connector_gets_only_the_checked_dns_answers() {
        let resolver = PublicResolver { inner: Answers(vec!["127.0.0.1:80".parse().unwrap(), "93.184.216.34:80".parse().unwrap(), "10.1.2.3:80".parse().unwrap()]), fixture: None };
        let config = ureq::Agent::config_builder().build();
        let addresses = resolver.resolve(&"https://pictures.example/a.png".parse().unwrap(), &config, NextTimeout::default()).unwrap();
        assert_eq!(&addresses[..], &["93.184.216.34:80".parse::<SocketAddr>().unwrap()]);
        let private = PublicResolver { inner: Answers(vec!["192.168.1.1:443".parse().unwrap()]), fixture: None };
        assert!(private.resolve(&"https://pictures.example/a.png".parse().unwrap(), &config, NextTimeout::default()).is_err());
    }
}
