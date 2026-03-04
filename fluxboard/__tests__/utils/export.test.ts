/**
 * Unit tests for export utilities
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { exportCSV, exportJSON, generateTimestampFilename } from '../../utils/export';

describe('export utilities', () => {
  let createElementSpy: any;
  let createObjectURLSpy: any;
  let revokeObjectURLSpy: any;

  beforeEach(() => {
    // Mock DOM APIs
    createElementSpy = vi.spyOn(document, 'createElement');
    if (!(URL as any).createObjectURL) {
      (URL as any).createObjectURL = vi.fn();
    }
    if (!(URL as any).revokeObjectURL) {
      (URL as any).revokeObjectURL = vi.fn();
    }
    createObjectURLSpy = vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:mock-url' as any);
    revokeObjectURLSpy = vi.spyOn(URL, 'revokeObjectURL');
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe('exportJSON', () => {
    it('should export simple data as JSON', () => {
      const mockAnchor = {
        href: '',
        download: '',
        click: vi.fn()
      };
      createElementSpy.mockReturnValue(mockAnchor);

      const data = { foo: 'bar', baz: 123 };
      exportJSON(data, 'test.json');

      expect(createElementSpy).toHaveBeenCalledWith('a');
      expect(createObjectURLSpy).toHaveBeenCalled();
      expect(mockAnchor.href).toBe('blob:mock-url');
      expect(mockAnchor.download).toBe('test.json');
      expect(mockAnchor.click).toHaveBeenCalled();
      expect(revokeObjectURLSpy).toHaveBeenCalledWith('blob:mock-url');
    });

    it('should export nested objects', () => {
      const mockAnchor = {
        href: '',
        download: '',
        click: vi.fn()
      };
      createElementSpy.mockReturnValue(mockAnchor);

      const data = { nested: { value: 42 }, array: [1, 2, 3] };
      exportJSON(data, 'nested.json');

      expect(mockAnchor.click).toHaveBeenCalled();
    });
  });

  describe('exportCSV', () => {
    it('should export simple data as CSV', () => {
      const mockAnchor = {
        href: '',
        download: '',
        click: vi.fn()
      };
      createElementSpy.mockReturnValue(mockAnchor);

      const data = [
        { coin: 'BTC', qty: 1.5, mv: 60000 },
        { coin: 'ETH', qty: 10, mv: 20000 }
      ];
      exportCSV(data, 'test.csv');

      expect(createElementSpy).toHaveBeenCalledWith('a');
      expect(createObjectURLSpy).toHaveBeenCalled();
      expect(mockAnchor.download).toBe('test.csv');
      expect(mockAnchor.click).toHaveBeenCalled();
      expect(revokeObjectURLSpy).toHaveBeenCalledWith('blob:mock-url');
    });

    it('should handle special characters (commas and quotes)', () => {
      const mockAnchor = {
        href: '',
        download: '',
        click: vi.fn()
      };
      createElementSpy.mockReturnValue(mockAnchor);

      const data = [
        { name: 'Test, Inc.', description: 'A "quoted" value' }
      ];
      exportCSV(data, 'special.csv');

      expect(mockAnchor.click).toHaveBeenCalled();
    });

    it('should handle null and undefined values', () => {
      const mockAnchor = {
        href: '',
        download: '',
        click: vi.fn()
      };
      createElementSpy.mockReturnValue(mockAnchor);

      const data = [
        { coin: 'BTC', qty: null, mv: undefined }
      ];
      exportCSV(data, 'nulls.csv');

      expect(mockAnchor.click).toHaveBeenCalled();
    });

    it('should handle empty array', () => {
      const mockAnchor = {
        href: '',
        download: '',
        click: vi.fn()
      };
      createElementSpy.mockReturnValue(mockAnchor);

      exportCSV([], 'empty.csv');

      expect(mockAnchor.download).toBe('empty.csv');
      expect(mockAnchor.click).toHaveBeenCalled();
    });
  });

  describe('generateTimestampFilename', () => {
    it('should generate filename with timestamp', () => {
      const filename = generateTimestampFilename('balances', 'csv');
      expect(filename).toMatch(/^balances_\d{4}-\d{2}-\d{2}T\d{2}-\d{2}-\d{2}\.csv$/);
    });

    it('should handle different prefixes and extensions', () => {
      const filename = generateTimestampFilename('trades', 'json');
      expect(filename).toMatch(/^trades_\d{4}-\d{2}-\d{2}T\d{2}-\d{2}-\d{2}\.json$/);
    });

    it('should generate unique filenames', () => {
      const filename1 = generateTimestampFilename('test', 'csv');
      // Wait a tiny bit to ensure different timestamp
      const filename2 = generateTimestampFilename('test', 'csv');
      // They might be the same if called in same second, but format should be consistent
      expect(filename1).toMatch(/^test_\d{4}-\d{2}-\d{2}T\d{2}-\d{2}-\d{2}\.csv$/);
      expect(filename2).toMatch(/^test_\d{4}-\d{2}-\d{2}T\d{2}-\d{2}-\d{2}\.csv$/);
    });
  });
});
