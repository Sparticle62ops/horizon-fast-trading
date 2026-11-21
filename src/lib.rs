//! Fast Trading for Horizon: repeat the selected villager trade fast.
//!
//! The 1:1 logic core, split out of the injected runtime module so
//! `cargo test` runs it standalone: the config (ticks, autofill, block
//! rules, price stop, new-offer stop), the runner state machine and the
//! repeat counter, the `/fasttrading` command language, and the speed
//! trade button's geometry and colors. The runtime module
//! (`src/runtime.rs`) inlines the same code
//! compile-equivalently for the game's injection, beside the game-touching
//! half (survey, clicks, autofill, the menu plumbing).
//!
//! Upstream: bendy1234/FastTrading, MIT, (c) 2020 ADudeCalledLeo; ported
//! 1:1 by sparticle, 2026.

use std::sync::Mutex;


/// The mod's id (its settings folder in the original).
pub const ID: &str = "fast_trading";
/// The command words the mod answers to.
pub const HEADS: [&str; 2] = ["fasttrading", "ftrade"];
/// One line of how the command reads, shown when it is not understood.
pub const USAGE: &str = "Usage: /fasttrading [status | ticks <seconds> | autofill default|strict | block damageable|unstackable|disabled | price-stop on-increase|on-change|disabled | new-offers on|off]";
/// The smallest `ticks_between_actions` the original accepts.
pub const MIN_TICKS: f64 = 0.025;

/// `AutofillBehavior`: how the payment slots are refilled between trades.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Autofill {
    /// Re-select the trade; the server moves the payment items in.
    Default,
    /// Empty the payment slots first, then fill them from the inventory.
    Strict,
}

/// `TradeBlockBehavior`: which trades the button refuses without the
/// override key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TradeBlock {
    Damageable,
    Unstackable,
    Disabled,
}

/// `PriceChangeStopBehavior`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PriceStop {
    OnIncrease,
    OnChange,
    Disabled,
}

impl PriceStop {
    /// `PriceChangeStopBehavior.shouldStop`.
    pub fn should_stop(self, old: u32, current: u32) -> bool {
        match self {
            PriceStop::OnIncrease => current > old,
            PriceStop::OnChange => old != current,
            PriceStop::Disabled => false,
        }
    }
}

/// `MerchantScreenHooks.State`, computed from the open merchant screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TradeState {
    NoSelection,
    OutOfStock,
    NoRoomForSellItem,
    CanPerform,
    NotEnoughBuyItems,
}

/// `ModConfig`.
#[derive(Debug)]
pub struct Config {
    /// Seconds between repeated actions (`ticksBetweenActions`, in ticks).
    pub ticks_between_actions: f64,
    pub autofill: Autofill,
    pub block: TradeBlock,
    pub price_stop: PriceStop,
    pub stop_on_new_offers: bool,
}

/// The button's phase: idle, filling the payment slots, or taking the result.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Autofill,
    Trade,
}

/// What the open merchant screen says right now.
struct Survey {
    state: TradeState,
    /// Whether the trade block rules refuse this trade without the override key.
    blocked: bool,
    /// The modified cost A count (demand and special price applied).
    cost_a: u32,
    offers_len: usize,
    /// The selected offer (`shopItem`): the session's selection hint.
    selection: usize,
}

/// `SpeedTradeTimer` and the button's phase, shared through the game's one thread.
struct Runner {
    config: Config,
    timer_active: bool,
    counter: f64,
    phase: Option<Phase>,
    trade_cost: u32,
    offer_count: usize,
    offer_idx: usize,
    alt_down: bool,
}

static RUNNER: Mutex<Runner> = Mutex::new(Runner {
    config: Config {
        ticks_between_actions: 1.0,
        autofill: Autofill::Default,
        block: TradeBlock::Damageable,
        price_stop: PriceStop::OnIncrease,
        stop_on_new_offers: false,
    },
    timer_active: false,
    counter: 0.0,
    phase: None,
    trade_cost: 0,
    offer_count: 0,
    offer_idx: 0,
    alt_down: false,
});

/// The merchant screen's own width (100 px wider than the 176-px menus).
const MERCHANT_WIDTH: f32 = 276.0;
/// The button sits 11 px from the panel's right edge, 36 px down (vanilla's
/// `leftPos + 247`, `topPos + 36`).
const BUTTON: [f32; 4] = [247.0, 36.0, 18.0, 20.0];
const GREEN: u32 = 0xFF2E_CC71;
const GREEN_HOVER: u32 = 0xFF58_D68A;
const YELLOW: u32 = 0xFFD4_B429;
const RED: u32 = 0xFFC0_392B;
const GREY: u32 = 0xFF55_5555;
const WHITE: u32 = 0xFFEB_EBEF;

/// The speed trade button's rect in GUI pixels, on a merchant panel `rect`.
pub fn button_rect(rect: [f32; 4]) -> [f32; 4] {
    let x = rect[0] + rect[2] - (MERCHANT_WIDTH - BUTTON[0]).min(rect[2] * 0.5);
    [x, rect[1] + BUTTON[1], BUTTON[2], BUTTON[3]]
}

/// One `/fasttrading` change.
#[derive(Debug, PartialEq)]
pub enum Action {
    Status,
    Ticks(f64),
    Autofill(Autofill),
    Block(TradeBlock),
    PriceStop(PriceStop),
    NewOffers(bool),
}

/// Parse a command line without its slash into an action. `head` is the
/// command's first word, `args` the rest.
pub fn parse(head: &str, args: &[&str]) -> Option<Action> {
    if !HEADS.contains(&head) {
        return None;
    }
    match args.first().copied() {
        None | Some("status") => Some(Action::Status),
        Some("ticks") => {
            let ticks = args.get(1)?.parse::<f64>().ok()?;
            ticks.ge(&MIN_TICKS).then(|| Action::Ticks(ticks))
        }
        Some("autofill") => match args.get(1).copied() {
            Some("default") => Some(Action::Autofill(Autofill::Default)),
            Some("strict") => Some(Action::Autofill(Autofill::Strict)),
            _ => None,
        },
        Some("block") => match args.get(1).copied() {
            Some("damageable") => Some(Action::Block(TradeBlock::Damageable)),
            Some("unstackable") => Some(Action::Block(TradeBlock::Unstackable)),
            Some("disabled") => Some(Action::Block(TradeBlock::Disabled)),
            _ => None,
        },
        Some("price-stop") => match args.get(1).copied() {
            Some("on-increase") => Some(Action::PriceStop(PriceStop::OnIncrease)),
            Some("on-change") => Some(Action::PriceStop(PriceStop::OnChange)),
            Some("disabled") => Some(Action::PriceStop(PriceStop::Disabled)),
            _ => None,
        },
        Some("new-offers") => match args.get(1).copied() {
            Some("on") => Some(Action::NewOffers(true)),
            Some("off") => Some(Action::NewOffers(false)),
            _ => None,
        },
        _ => None,
    }
}

/// Apply one action and return the line to show for it.
pub fn apply(action: Action) -> String {
    let mut r = RUNNER.lock().unwrap();
    match action {
        Action::Ticks(t) => {
            r.config.ticks_between_actions = t;
            format!("Speed trade interval: {t:.3}s")
        }
        Action::Autofill(a) => {
            r.config.autofill = a;
            format!("Autofill: {a:?}").to_lowercase()
        }
        Action::Block(b) => {
            r.config.block = b;
            format!("Trade block: {b:?}").to_lowercase()
        }
        Action::PriceStop(p) => {
            r.config.price_stop = p;
            format!("Price-change stop: {p:?}").to_lowercase()
        }
        Action::NewOffers(on) => {
            r.config.stop_on_new_offers = on;
            format!("Stop on new offers: {}", if on { "on" } else { "off" })
        }
        Action::Status => status(&r),
    }
}

/// The status line `/fasttrading` shows with no argument.
fn status(r: &Runner) -> String {
    let c = &r.config;
    format!(
        "Fast Trading: ticks {:.3}s, autofill {:?}, block {:?}, price-stop {:?}, new-offers {}; timer {}",
        c.ticks_between_actions,
        c.autofill,
        c.block,
        c.price_stop,
        if c.stop_on_new_offers { "on" } else { "off" },
        if r.timer_active { "running" } else { "idle" },
    )
    .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_command() {
        assert_eq!(parse("fasttrading", &[]), Some(Action::Status));
        assert_eq!(parse("ftrade", &["ticks", "0.5"]), Some(Action::Ticks(0.5)));
        assert_eq!(parse("ftrade", &["ticks", "0.01"]), None);
        assert_eq!(
            parse("fasttrading", &["autofill", "strict"]),
            Some(Action::Autofill(Autofill::Strict))
        );
        assert_eq!(
            parse("fasttrading", &["price-stop", "on-change"]),
            Some(Action::PriceStop(PriceStop::OnChange))
        );
        assert_eq!(parse("other", &["ticks", "1"]), None);
    }

    #[test]
    fn price_stop_follows_the_original() {
        assert!(PriceStop::OnIncrease.should_stop(1, 2));
        assert!(!PriceStop::OnIncrease.should_stop(2, 2));
        assert!(PriceStop::OnChange.should_stop(2, 1));
        assert!(!PriceStop::Disabled.should_stop(1, 5));
    }

    #[test]
    fn the_button_sits_at_the_panels_right_edge() {
        let rect = [100.0, 50.0, MERCHANT_WIDTH, 166.0];
        let [x, y, w, h] = button_rect(rect);
        assert_eq!(x, 100.0 + 247.0);
        assert_eq!(y, 86.0);
        assert_eq!((w, h), (18.0, 20.0));
    }
}
