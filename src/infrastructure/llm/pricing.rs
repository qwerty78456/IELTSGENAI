//! Gemini paid-tier prices, standard service tier, in USD per million tokens
//! (checked on ai.google.dev/gemini-api/docs/pricing, 2026-09-28).
//!
//! The 3.8 models launched at introductory prices that double on 2027-01-01;
//! each request is priced at the rate in force when it is made, so the
//! ledger keeps what was actually billed. A model missing here is recorded
//! with its tokens and flagged as unpriced rather than guessed.

/// USD per million tokens, which is the same number as µUSD per token.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rates {
    pub input: f64,
    pub cached_input: f64,
    /// Text, audio or thinking output.
    pub output: f64,
}

struct Price {
    model: &'static str,
    intro: Rates,
    list: Rates,
}

/// 2027-01-01T00:00:00Z: the end of the introductory 3.8 prices.
const INTRO_ENDS_SECS: i64 = 1_798_761_600;

const PRICES: [Price; 3] = [
    Price {
        model: "gemini-3.8-flash",
        intro: Rates {
            input: 0.75,
            cached_input: 0.075,
            output: 3.75,
        },
        list: Rates {
            input: 1.50,
            cached_input: 0.15,
            output: 7.50,
        },
    },
    Price {
        model: "gemini-3.8-flash-tts",
        intro: Rates {
            input: 0.50,
            cached_input: 0.125,
            output: 9.00,
        },
        list: Rates {
            input: 1.00,
            cached_input: 0.25,
            output: 18.00,
        },
    },
    Price {
        model: "gemini-3.8-flash-lite-tts",
        intro: Rates {
            input: 0.50,
            cached_input: 0.125,
            output: 6.00,
        },
        list: Rates {
            input: 1.00,
            cached_input: 0.25,
            output: 12.00,
        },
    },
];

/// The rates for `model` at `at_secs` (Unix time), if the model is known.
pub fn rates_for(model: &str, at_secs: i64) -> Option<Rates> {
    PRICES
        .iter()
        .find(|price| price.model == model)
        .map(|price| {
            if at_secs < INTRO_ENDS_SECS {
                price.intro
            } else {
                price.list
            }
        })
}

/// The cost in µUSD of one response's tokens. `input` includes `cached`.
pub fn cost_micro_usd(rates: Rates, input: u64, cached: u64, output: u64, thinking: u64) -> u64 {
    let cached = cached.min(input);
    let micro_usd = (input - cached) as f64 * rates.input
        + cached as f64 * rates.cached_input
        + (output + thinking) as f64 * rates.output;
    micro_usd.round() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_double_on_the_first_of_january_2027() {
        let before = rates_for("gemini-3.8-flash-tts", INTRO_ENDS_SECS - 1).unwrap();
        let after = rates_for("gemini-3.8-flash-tts", INTRO_ENDS_SECS).unwrap();
        assert_eq!(before.output, 9.00);
        assert_eq!(after.output, 18.00);
        assert!(rates_for("gemini-flash-latest", 0).is_none());
    }

    #[test]
    fn cached_and_thinking_tokens_use_their_own_rates() {
        let rates = rates_for("gemini-3.8-flash", 0).unwrap();
        // 1,000 fresh input at 0.75, 1,000 cached at 0.075, 400 output + 600 thinking at 3.75.
        assert_eq!(
            cost_micro_usd(rates, 2_000, 1_000, 400, 600),
            750 + 75 + 3_750
        );
        // 24,000 audio tokens (12.5 minutes at 32 tokens/s) on 3.8 Flash TTS: $0.216 now.
        let tts = rates_for("gemini-3.8-flash-tts", 0).unwrap();
        assert_eq!(cost_micro_usd(tts, 0, 0, 24_000, 0), 216_000);
    }
}
