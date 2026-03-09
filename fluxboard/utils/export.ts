/**
 * Export utilities for CSV and JSON file downloads
 * Zero external dependencies - uses native browser APIs
 */

/**
 * Export data as JSON file
 * @param data - Any JSON-serializable data
 * @param filename - Output filename (with .json extension)
 */
export function exportJSON(data: any, filename: string): void {
  const blob = new Blob([JSON.stringify(data, null, 2)], {
    type: 'application/json'
  });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}

/**
 * Export array of objects as CSV file
 * @param data - Array of objects (must have consistent keys)
 * @param filename - Output filename (with .csv extension)
 */
export function exportCSV(data: any[], filename: string): void {
  if (!data || data.length === 0) {
    // Export empty CSV with no headers
    const blob = new Blob([''], { type: 'text/csv' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = filename;
    a.click();
    URL.revokeObjectURL(url);
    return;
  }

  // Extract keys from first object
  const keys = Object.keys(data[0]);
  const header = keys.join(',');

  // Build CSV rows with proper escaping
  const rows = data.map(row =>
    keys.map(k => {
      const val = row[k];

      // Handle null/undefined
      if (val === null || val === undefined) {
        return '';
      }

      // Convert to string
      const strVal = String(val);

      // Escape if contains comma, quote, or newline
      if (strVal.includes(',') || strVal.includes('"') || strVal.includes('\n')) {
        return `"${strVal.replace(/"/g, '""')}"`;
      }

      return strVal;
    }).join(',')
  );

  const csv = [header, ...rows].join('\n');
  const blob = new Blob([csv], { type: 'text/csv' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}

/**
 * Generate timestamped filename
 * @param prefix - Filename prefix (e.g., "balances")
 * @param ext - File extension without dot (e.g., "csv" or "json")
 * @returns Filename like "balances_2025-10-20T14-30-45.csv"
 */
export function generateTimestampFilename(prefix: string, ext: string): string {
  const timestamp = new Date()
    .toISOString()
    .replace(/[:.]/g, '-')
    .slice(0, 19); // YYYY-MM-DDTHH-MM-SS
  return `${prefix}_${timestamp}.${ext}`;
}
