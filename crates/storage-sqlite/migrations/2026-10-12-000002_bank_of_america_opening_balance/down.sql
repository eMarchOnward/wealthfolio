UPDATE import_templates
SET config = '{"fieldMappings":{"date":"Date","activityType":"Type","amount":"Amount","comment":"Description"},"activityMappings":{"DEPOSIT":["Credit"],"WITHDRAWAL":["Debit"],"TRANSFER_OUT":["Transfer to Vanguard"],"_SKIP_":["Beginning balance"]},"symbolMappings":{},"accountMappings":{},"symbolMappingMeta":{},"parseConfig":{"delimiter":",","dateFormat":"MM/DD/YYYY","decimalSeparator":".","thousandsSeparator":",","hasHeaderRow":true,"skipTopRows":6,"skipBottomRows":0}}',
    updated_at = CURRENT_TIMESTAMP
WHERE id = 'system_bank_of_america';
