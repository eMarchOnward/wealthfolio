-- Bank of America checking/savings transaction export (CSV download).
-- The export has no transaction type; the import flow adds a "Type" column
-- (Credit / Debit / Beginning balance / Transfer to Vanguard) from the amount
-- sign and description
-- (apps/frontend/src/pages/activity/import/utils/bank-of-america-adjustments.ts).
-- The summary block above the transactions is skipped (5 lines + blank line).
INSERT OR REPLACE INTO import_templates (id, name, scope, kind, source_system, config_version, config, created_at, updated_at)
VALUES (
    'system_bank_of_america',
    'Bank of America',
    'SYSTEM',
    'CSV_ACTIVITY',
    '',
    1,
    '{"fieldMappings":{"date":"Date","activityType":"Type","amount":"Amount","comment":"Description"},"activityMappings":{"DEPOSIT":["Credit"],"WITHDRAWAL":["Debit"],"TRANSFER_OUT":["Transfer to Vanguard"],"_SKIP_":["Beginning balance"]},"symbolMappings":{},"accountMappings":{},"symbolMappingMeta":{},"parseConfig":{"delimiter":",","dateFormat":"MM/DD/YYYY","decimalSeparator":".","thousandsSeparator":",","hasHeaderRow":true,"skipTopRows":6,"skipBottomRows":0}}',
    CURRENT_TIMESTAMP,
    CURRENT_TIMESTAMP
);
