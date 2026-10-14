-- Fidelity NetBenefits workplace-plan (401(k)) transaction history (CSV download).
-- The export has no symbol or price. Its header is the third non-empty line, below
-- the plan name and date range (headerRowIndex counts non-empty lines, whatever
-- the line endings). The import flow drops those two lines, adds "Symbol" (from the
-- fund name) and "Price" (amount / shares) columns, and puts a "Contribution
-- deposit" row before each payroll contribution
-- (apps/frontend/src/pages/activity/import/utils/fidelity-adjustments.ts).
INSERT OR REPLACE INTO import_templates (id, name, scope, kind, source_system, config_version, config, created_at, updated_at)
VALUES (
    'system_fidelity_netbenefits',
    'Fidelity NetBenefits (401k)',
    'SYSTEM',
    'CSV_ACTIVITY',
    '',
    1,
    '{"fieldMappings":{"date":"Date","activityType":"Transaction Type","symbol":"Symbol","quantity":"Shares/Unit","unitPrice":"Price","amount":"Amount","comment":"Investment"},"activityMappings":{"BUY":["Contributions"],"DEPOSIT":["Contribution deposit"],"TRANSFER_IN":["Transfer In"]},"symbolMappings":{},"accountMappings":{},"symbolMappingMeta":{},"parseConfig":{"delimiter":",","dateFormat":"MM/DD/YYYY","decimalSeparator":".","thousandsSeparator":",","hasHeaderRow":true,"headerRowIndex":2,"skipTopRows":0,"skipBottomRows":0}}',
    CURRENT_TIMESTAMP,
    CURRENT_TIMESTAMP
);
