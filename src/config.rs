use std::io::{Read, Write};

use pddb::Pddb;

use crate::VaultMode;

pub(crate) const DC34_DICT: &str = "dc34";
pub(crate) const DC34_SECRET: &str = "k0";
pub(crate) const DC34_TOUR: &str = "tour";
pub(crate) const DC34_TOKEN_TOUR: &str = "tokentour";
pub(crate) const DC34_BADGE: &str = "badge";

#[derive(Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum BadgeType {
    Human = 5,
    Goon = 3,
    Village = 6,
    CtfContest = 1,
    Uber = 0,
    Community = 2,
    Other = 4,
    None = 7,
}
impl TryFrom<u8> for BadgeType {
    type Error = u8;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            3 => Ok(BadgeType::Human),
            5 => Ok(BadgeType::Goon),
            6 => Ok(BadgeType::Village),
            1 => Ok(BadgeType::CtfContest),
            0 => Ok(BadgeType::Uber),
            2 => Ok(BadgeType::Community),
            4 => Ok(BadgeType::Other),
            7 => Ok(BadgeType::None),
            n => Err(n),
        }
    }
}

/// Structure for tracking global shared state. Everything in here
/// needs to be suitable for sticking in an Arc/Mutex
pub(crate) struct GlobalConfig {
    is_developer: bool,
    badge_attached: bool,
    was_cold_boot: bool,
    k0: [u8; 32],
    skip_tour: bool,
    skip_token_tour: bool,
    badge_type: BadgeType,
}

impl GlobalConfig {
    pub fn init() -> (Self, VaultMode) {
        let xns = xous_names::XousNames::new().unwrap();
        let keystore = keystore::Keystore::new(&xns);
        let is_developer = keystore.is_developer().expect("couldn't query developer mode");
        let pddb = pddb::Pddb::new();

        // TODO: replace with a function that actually checks the attachment pins
        let badge_attached = true;
        // TODO: replace with a function that actually checks cold boot status
        let was_cold_boot = true;
        // initialize the system state from the PDDB
        let mut k0 = [0u8; 32];
        let k0_len = read_pddb(&pddb, DC34_SECRET, &mut k0);
        log::info!("k0_len {}, k0 {:x?}", k0_len, k0);

        let mut skip_tour_buf = [0u8; 1];
        read_pddb(&pddb, DC34_TOUR, &mut skip_tour_buf);
        let skip_tour = skip_tour_buf[0] != 0;
        log::info!("skip_tour {:?},  {:x?}", skip_tour, skip_tour_buf);

        let mut skip_token_tour_buf = [0u8; 1];
        read_pddb(&pddb, DC34_TOKEN_TOUR, &mut skip_token_tour_buf);
        let skip_token_tour = skip_token_tour_buf[0] != 0;
        log::info!("skip_token_tour {:?},  {:x?}", skip_token_tour, skip_token_tour_buf);

        let mut badge_code = [BadgeType::None as u8; 1];
        let badge_code_len = read_pddb(&pddb, DC34_BADGE, &mut badge_code);
        let badge_type = if badge_code_len == 0 {
            BadgeType::None
        } else {
            BadgeType::try_from(badge_code[0]).unwrap_or(BadgeType::None)
        };

        let initial_mode = if badge_type == BadgeType::None {
            VaultMode::FactoryTest
        } else if badge_attached {
            if skip_tour || !was_cold_boot { VaultMode::Idle } else { VaultMode::Tour }
        } else {
            if skip_token_tour { VaultMode::Password } else { VaultMode::TokenTour }
        };

        (
            GlobalConfig {
                is_developer,
                badge_attached,
                was_cold_boot,
                k0,
                skip_tour,
                skip_token_tour,
                badge_type,
            },
            initial_mode,
        )
    }

    pub fn set_skip_tour(&mut self, state: bool) -> VaultMode {
        let pddb = pddb::Pddb::new();
        let mut key = pddb
            .get(DC34_DICT, DC34_TOUR, None, true, true, Some(1), None::<fn()>)
            .expect("couldn't get PDDB key");
        if state {
            key.write(&[1]).ok();
        } else {
            key.write(&[0]).ok();
        }
        if self.badge_attached { VaultMode::Idle } else { VaultMode::Password }
    }

    pub fn is_badge_attached(&self) -> bool { self.badge_attached }
}

pub fn read_pddb(pddb: &Pddb, key: &str, buf: &mut [u8]) -> usize {
    let mut key = pddb
        .get(DC34_DICT, key, None, true, true, Some(buf.len()), None::<fn()>)
        .expect("couldn't get PDDB key");
    key.read(buf).expect("couldn't read key")
}

/// The side-effect call allows us to set global mutable state on disk
/// without having to actually share the GlobalConfig object
pub fn side_effect_skip_token_tour(state: bool) {
    // disable repeating the tour - show it only once
    let pddb = pddb::Pddb::new();
    let mut key = pddb
        .get(
            crate::config::DC34_DICT,
            crate::config::DC34_TOKEN_TOUR,
            None,
            true,
            true,
            Some(1),
            None::<fn()>,
        )
        .expect("couldn't get PDDB key");
    if state {
        key.write(&[1]).ok();
    } else {
        key.write(&[0]).ok();
    }
}
