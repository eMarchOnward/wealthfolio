-- Bank of America: the statement's opening balance ("Beginning balance") is
-- imported as a deposit instead of being dropped. The import flow skips it with
-- a note so it can be restored for the first statement imported only
-- (apps/frontend/src/pages/activity/import/utils/bank-of-america-adjustments.ts).
UPDATE import_templates
SET config = '{"fieldMappings":{"date":"Date","activityType":"Type","amount":"Amount","comment":"Description"},"activityMappings":{"DEPOSIT":["Credit","Beginning balance"],"WITHDRAWAL":["Debit"],"TRANSFER_OUT":["Transfer to Vanguard"]},"symbolMappings":{},"accountMappings":{},"symbolMappingMeta":{},"parseConfig":{"delimiter":",","dateFormat":"MM/DD/YYYY","decimalSeparator":".","thousandsSeparator":",","hasHeaderRow":true,"skipTopRows":6,"skipBottomRows":0}}',
    updated_at = CURRENT_TIMESTAMP
WHERE id = 'system_bank_of_america';
