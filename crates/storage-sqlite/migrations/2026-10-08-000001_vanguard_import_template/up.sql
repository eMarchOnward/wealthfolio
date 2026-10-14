-- Vanguard "Transaction History" custom activity report (CSV export).
-- Settlement-fund sweeps, Treasury bills, matched account-transfer pairs, and
-- legacy mutual-fund-account rows are adjusted in the frontend import flow
-- (apps/frontend/src/pages/activity/import/utils/vanguard-adjustments.ts).
INSERT OR REPLACE INTO import_templates (id, name, scope, kind, source_system, config_version, config, created_at, updated_at)
VALUES (
    'system_vanguard',
    'Vanguard',
    'SYSTEM',
    'CSV_ACTIVITY',
    '',
    1,
    '{"fieldMappings":{"date":["Trade date","Settlement date"],"activityType":"Type","symbol":"Symbol","quantity":"Quantity","unitPrice":"Price","fee":"Commission & fees**","amount":"Amount","comment":"Name"},"activityMappings":{"BUY":["Buy","Buy (exchange)","Reinvestment","Reinvestment (LT gain)","Reinvestment (ST gain)"],"SELL":["Sell","Sell (exchange)","Corp Action (Redemption)"],"DIVIDEND":["Dividend","Capital gain (LT)","Capital gain (ST)"],"INTEREST":["Interest"],"DEPOSIT":["Rollover (incoming)","Sweep out"],"WITHDRAWAL":["Withdrawal","Transfer (Outgoing)","Sweep in"],"TRANSFER_IN":["Funds Received","Contribution","Conversion","Recharacterization (incoming)","Share Conversion (incoming)","TRANSFER FROM *"],"TRANSFER_OUT":["TRANSFER TO *","Recharacterization (outgoing)","Share Conversion (outgoing)"]},"symbolMappings":{},"accountMappings":{},"symbolMappingMeta":{},"parseConfig":{"delimiter":",","dateFormat":"M/d/yyyy","decimalSeparator":".","thousandsSeparator":",","hasHeaderRow":true,"skipTopRows":3,"skipBottomRows":0}}',
    CURRENT_TIMESTAMP,
    CURRENT_TIMESTAMP
);
