//! Hi5-Bot daemon entry point.
//!
//! Usage:
//!   hi5bot                # long-running: wakes daily at 15:30 America/Toronto
//!   hi5bot --once         # run a single evaluation tick now and exit
//!   hi5bot --once --dry-run  # compute but do not place orders
//!
//! Data directory: `$HI5BOT_DATA_DIR` or `./data` (holds config.toml,
//! tokens.json, state.json). On the Synology container this is the mounted
//! `/app/data` volume.

use chrono::Utc;
use chrono_tz::America::Toronto;
use hi5bot::{Error, auth::TokenStore, calendar, config, engine, state_store::MonthlyStateStore};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    let once = args.iter().any(|a| a == "--once");
    let dry_run = args.iter().any(|a| a == "--dry-run");

    if args.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!("hi5bot [--once] [--dry-run]");
        return Ok(());
    }

    let settings = config::Settings::load()?;
    let (eval_h, eval_m) = calendar::parse_eval_time(&settings.eval_time)
        .ok_or_else(|| anyhow::anyhow!("invalid eval_time '{}'", settings.eval_time))?;
    tracing::info!(
        "loaded config; accounts={} resp/tfsa; eval={:02}:{:02} America/Toronto; M={}",
        settings.accounts().len(),
        eval_h,
        eval_m,
        settings.safety_buffer_m
    );

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let tokens = TokenStore::load(config::tokens_path())?;
    let state_store = MonthlyStateStore::load(config::state_path())?;

    if once {
        let now = Utc::now().with_timezone(&Toronto);
        match engine::run_tick(
            &settings,
            &tokens,
            &state_store,
            &http,
            now.naive_local(),
            dry_run,
        )
        .await
        {
            Ok(summaries) => {
                for s in &summaries {
                    tracing::info!(
                        "account={} signal={:?} orders_placed={}",
                        s.account,
                        s.signal,
                        s.orders_placed
                    );
                }
            }
            Err(e) => {
                tracing::error!("tick failed: {e:?}");
                return Err(anyhow::anyhow!(e.to_string()));
            }
        }
        return Ok(());
    }

    loop {
        let now = Utc::now().with_timezone(&Toronto);
        let next = calendar::next_eval(now, eval_h, eval_m)
            .ok_or_else(|| anyhow::anyhow!("could not compute next eval time"))?;
        let dur = (next - now)
            .to_std()
            .unwrap_or(std::time::Duration::from_secs(60));
        tracing::info!("next eval at {} (sleeping {}s)", next, dur.as_secs());
        tokio::time::sleep(dur).await;

        let now = Utc::now().with_timezone(&Toronto);
        match engine::run_tick(
            &settings,
            &tokens,
            &state_store,
            &http,
            now.naive_local(),
            dry_run,
        )
        .await
        {
            Ok(summaries) => {
                for s in &summaries {
                    tracing::info!(
                        "account={} signal={:?} orders_placed={}",
                        s.account,
                        s.signal,
                        s.orders_placed
                    );
                }
            }
            Err(Error::UsdCashExhausted) | Err(Error::SettlementNotCurrencyOfTransaction(_)) => {
                tracing::error!("hard-abort condition; exiting non-zero");
                std::process::exit(1);
            }
            Err(e) => {
                tracing::error!("tick error (will retry next eval): {e:?}");
            }
        }
    }
}
