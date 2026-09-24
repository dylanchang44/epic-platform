# Synthetic fixtures

All names, dates, quantities, and amounts here were invented for testing.
No brokerage exports were copied. `schwab/Example-Positions.csv` is the minimum
directory layout: a single positions CSV directly inside the configured folder.

Expected: four rows (including cash), signed market-value total USD 1,552.75.
The option's exported market value is used directly, not quantity × price;
the contract multiplier is already reflected in the export. Cost-basis absence
stays absent. Balances, transactions and realized gain/loss are not required.

`research/Stage3-Positions.csv` adds six invented positions (USD 425 total):
lowercase NVDA and MSFT for registry matches, ZZZZ for an unmatched ordinary
stock, and ETF, option and cash rows for unsupported instruments. Company symbols
are real where needed for matching; quantities and values are entirely synthetic.
The original Stage 2 fixture is unchanged.

`research/forecast.json` is an independently invented, minimal Stock Analysis
SvelteKit/devalue response using the contract decoded by ConsensX `market.rs`
at commit `99c8e68dd5e5bf830361a6fcbb50c1a5921f9b6c`. All figures are test values,
not financial research. Each integer inside an object/array is a table reference;
numeric table entries themselves are literal numbers. Offline tests serve this
fixture at `/stocks/nvda/forecast/__data.json` on an ephemeral loopback port.
