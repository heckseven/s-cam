use std::io::{Read, Write};

use bao1x_api::{IoSetup, IoxPort, IoxValue};
use dc34_api::BadgeType;
use pddb::Pddb;

use crate::VaultMode;

pub(crate) const DC34_DICT: &str = "dc34";
pub(crate) const DC34_SECRET: &str = "k0";
pub(crate) const DC34_TOUR: &str = "tour";
pub(crate) const DC34_TOKEN_TOUR: &str = "tokentour";
pub(crate) const DC34_BADGE: &str = "badge";

pub(crate) const SAO_GPIO: [(IoxPort, u8); 4] =
    [(IoxPort::PC, 5), (IoxPort::PC, 6), (IoxPort::PC, 14), (IoxPort::PC, 15)];

/// Structure for tracking global shared state. Everything in here
/// needs to be suitable for sticking in an Arc/Mutex
pub(crate) struct GlobalConfig {
    is_developer: bool,
    badge_attached: bool,
    attachment_match: bool,
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

        let mut flags = keystore.get_flags().expect("couldn't get flags");
        let was_cold_boot = !flags.warm_boot();
        // now set the warm boot field as true - so if we go into deep sleep we can detect that
        flags.set_warm_boot(true);
        keystore.set_flags(flags).unwrap();

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
        let stored_type = BadgeType::try_from(badge_code[0]).unwrap_or(BadgeType::None);
        let badge_type_measured = read_badgetype_pins();
        let badge_type = if badge_code_len == 0 {
            // none stored, init from pins
            badge_type_measured
        } else {
            if stored_type == BadgeType::None {
                // if the stored type is None, check and see if something new has been mated
                badge_type_measured
            } else {
                // just return the previously stored type, ignore the pins - so if the badge is re-mated, it
                // doesn't overwrite the factory-initialized type
                stored_type
            }
        };
        let attachment_match = stored_type == badge_type_measured;
        let badge_attached = badge_type_measured != BadgeType::None;

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
                attachment_match,
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

pub fn read_badgetype_pins() -> BadgeType {
    let iox = bao1x_api::iox::IoxHal::new();
    for &(port, pin) in SAO_GPIO[..3].iter() {
        iox.setup_pin(
            port,
            pin,
            Some(bao1x_api::IoxDir::Input),
            Some(bao1x_api::IoxFunction::Gpio),
            Some(bao1x_api::IoxEnable::Enable),
            Some(bao1x_api::IoxEnable::Enable),
            None,
            None,
        );
    }
    let mut bits: u8 = 0;
    for (i, &(port, pin)) in SAO_GPIO[..3].iter().enumerate() {
        if iox.get_gpio_pin_value(port, pin) == IoxValue::High {
            bits |= 1 << (i as u8);
        }
    }
    BadgeType::try_from(bits).unwrap_or(BadgeType::None)
}
