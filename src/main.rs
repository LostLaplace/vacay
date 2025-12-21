use chrono::{self, Datelike, NaiveDate, Weekday};
use clap::Parser;
use color_print::{ceprintln, cprintln};
use serde::Deserialize;
use std::fs;
use std::path::Path;
use std::process::exit;
use tabled::{Table, Tabled};
use toml::value::{Date, Datetime};
use toml_datetime_compat::FromToTomlDateTime;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(long = "accrual", short = 'a')]
    pto_hrs_per_wk: Option<f32>,
    #[arg(long = "bank", short = 'b')]
    pto_bank: Option<f32>,
    #[arg(long = "config", short = 'c', default_value = "config.toml")]
    path_to_config: Box<Path>,
    #[arg(long = "sched", short = 's')]
    path_to_sched: Box<Path>,
    #[arg(long = "verbose")]
    verbose: bool,
}

#[derive(Deserialize, Debug)]
struct Config {
    ptoHoursPerWeek: Option<f32>,
    ptoBank: Option<f32>,
    holidays: Vec<Date>,
}

#[derive(Deserialize, Debug)]
struct Schedule {
    vacations: Vec<Vacation>,
}

#[derive(Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Vacation {
    #[serde(with = "toml_datetime_compat")]
    start: NaiveDate,
    #[serde(with = "toml_datetime_compat")]
    end: NaiveDate,
    name: Option<String>,
}

#[derive(Tabled)]
struct VacationRow {
    #[tabled(rename = "Vacation")]
    name: String,
    #[tabled(rename = "Start")]
    start: NaiveDate,
    #[tabled(rename = "End")]
    end: NaiveDate,
    #[tabled(rename = "Days")]
    days: usize,
    #[tabled(rename = "Hours")]
    hours: i32,
    #[tabled(rename = "Status")]
    status: String,
}

fn main() {
    let mut args = Args::parse();

    if !args.path_to_config.exists() {
        println!("Error: config file with list of Garmin holidays is required");
        exit(1);
    }
    let config_contents = fs::read_to_string(args.path_to_config)
        .expect("The config file should exist and be readable.");
    let config: Config =
        toml::from_str(&config_contents).expect("The config file should be parseable.");

    if args.pto_hrs_per_wk.is_none() {
        args.pto_hrs_per_wk = Some(config.ptoHoursPerWeek.expect("Error: Missing accrual rate"));
    }
    if args.pto_bank.is_none() {
        args.pto_bank = Some(config.ptoBank.expect("Error: Missing banked PTO value"));
    }

    cprintln!("Let's go on <green><i>vacation</i></green>!");
    ceprintln!("PTO bank:    <blue>{}</blue> hours", args.pto_bank.unwrap());
    ceprintln!(
        "PTO accrual: <blue>{}</blue> hours / week",
        args.pto_hrs_per_wk.unwrap()
    );
    ceprintln!(
        "Garmin holidays in config: <blue>{}</blue> days",
        config.holidays.len()
    );

    let sched_contents =
        fs::read_to_string(args.path_to_sched).expect("The schedule file should be readable.");
    let sched: Schedule =
        toml::from_str(&sched_contents).expect("The schedule file should be parseable.");

    let mut vacations = sched.vacations;
    vacations.sort_unstable();

    let today = chrono::Local::now().date_naive();
    vacations.retain(|vac| vac.end > today);

    if vacations.len() == 0 {
        println!("No vacations in your schedule :(");
        exit(0);
    }

    // Convert holidays to NaiveDate for easier comparison
    let holidays: Vec<NaiveDate> = config
        .holidays
        .iter()
        .map(|d| NaiveDate::from_ymd_opt(d.year.into(), d.month.into(), d.day.into()).unwrap())
        .collect();

    // Start with current PTO balance as f32 for fractional hours
    let mut pto_balance = args.pto_bank.unwrap();
    let pto_accrual_per_week = args.pto_hrs_per_wk.unwrap();

    // Get current date and advance to next Sunday (not including today)
    let mut current_date = today;
    // First advance past today
    current_date = current_date.succ_opt().unwrap();
    // Then find the next Sunday
    while current_date.weekday() != Weekday::Sun {
        current_date = current_date.succ_opt().unwrap();
    }

    // Process each vacation in chronological order
    let mut rows = Vec::new();

    for vacation in &vacations {
        // Calculate PTO days needed (inclusive of start and end date)
        // Skip weekends and holidays
        let mut pto_days_needed = 0;
        let mut date = vacation.start;

        while date <= vacation.end {
            // Check if it's a weekday (Monday = 1, Sunday = 7)
            let weekday = date.weekday();
            if weekday != Weekday::Sat && weekday != Weekday::Sun {
                // Check if it's not a holiday
                if !holidays.contains(&date) {
                    pto_days_needed += 1;
                }
            }
            date = date.succ_opt().unwrap();
        }

        // Calculate PTO hours needed (assuming 8 hours per day)
        let pto_hours_needed = (pto_days_needed * 8) as f32;

        // Simulate time passing from current_date to vacation start
        // Add PTO accrual for each Sunday that passes
        while current_date < vacation.start {
            // Add PTO accrual on Sunday
            pto_balance += pto_accrual_per_week;
            if args.verbose {
                cprintln!(
                    "<dim>Accrued PTO on {:?}: +{} hours (balance: {:.2})</dim>",
                    current_date,
                    pto_accrual_per_week,
                    pto_balance
                );
            }
            // Advance to next Sunday
            for _ in 0..7 {
                current_date = current_date.succ_opt().unwrap();
            }
        }

        // Format vacation name
        let name = vacation.name.as_deref().unwrap_or("Unnamed").to_string();

        // Check if we have enough PTO for this vacation
        let status = if pto_balance >= pto_hours_needed {
            // Deduct PTO for the vacation
            pto_balance -= pto_hours_needed;
            "✓".to_string()
        } else {
            "✗".to_string()
        };

        // Add row to table
        rows.push(VacationRow {
            name,
            start: vacation.start,
            end: vacation.end,
            days: pto_days_needed,
            hours: pto_hours_needed as i32,
            status,
        });

        // Handle PTO accrual during the vacation (on Sundays)
        let mut vacation_date = vacation.start;
        while vacation_date <= vacation.end {
            if vacation_date.weekday() == Weekday::Sun {
                pto_balance += pto_accrual_per_week;
                if args.verbose {
                    cprintln!(
                        "<dim>Accrued PTO during vacation on {:?}: +{} hours (balance: {:.2})</dim>",
                        vacation_date,
                        pto_accrual_per_week,
                        pto_balance
                    );
                }
            }
            vacation_date = vacation_date.succ_opt().unwrap();
        }
        // Update current_date to day after vacation
        current_date = vacation.end.succ_opt().unwrap();
        // Advance to next Sunday
        while current_date.weekday() != Weekday::Sun {
            current_date = current_date.succ_opt().unwrap();
        }
    }

    // Print the formatted table
    let table = Table::new(rows);
    println!("{}", table);
    cprintln!("\n<blue>Final PTO balance: {:.2} hours</blue>", pto_balance);
}
