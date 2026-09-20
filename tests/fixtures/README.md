# Synthetic Schwab fixture

All names, dates, quantities, and amounts here were invented for testing.
No brokerage exports were copied. `schwab/Example-Positions.csv` is the minimum
directory layout: a single positions CSV directly inside the configured folder.

Expected: four rows (including cash), signed market-value total USD 1,552.75.
The option's exported market value is used directly, not quantity × price;
the contract multiplier is already reflected in the export. Cost-basis absence
stays absent. Balances, transactions and realized gain/loss are not required.
