/**
 * Parameter validation utility.
 *
 * Validates strategy parameters against schema rules including:
 * - Type checking (bool, int, float, select)
 * - Range validation (min/max bounds)
 * - Enum validation (select options)
 * - User-friendly error messages
 */

import type { ParamDef, ParamSchema, ValidationResult, ValidationErrors } from '../types';

/**
 * Validate a single parameter value against its schema definition.
 *
 * @param key - Parameter key (e.g., "bot_on", "qty")
 * @param value - Value to validate (string from input)
 * @param paramDef - Schema definition for this parameter
 * @returns ValidationResult with valid flag and optional error message
 */
export function validateParam(
  key: string,
  value: any,
  paramDef: ParamDef
): ValidationResult {
  // Trim whitespace from string values
  const trimmedValue = typeof value === 'string' ? value.trim() : value;

  // Empty string is invalid
  if (trimmedValue === '' || trimmedValue === null || trimmedValue === undefined) {
    return {
      valid: false,
      error: `${key} is required`
    };
  }

  try {
    switch (paramDef.type) {
      case 'bool':
      case 'select':
        return validateSelect(key, trimmedValue, paramDef);

      case 'int':
        return validateInt(key, trimmedValue, paramDef);

      case 'float':
        return validateFloat(key, trimmedValue, paramDef);

      default:
        return {
          valid: false,
          error: `Unknown param type '${paramDef.type}' for ${key}`
        };
    }
  } catch (error) {
    return {
      valid: false,
      error: `Validation error for ${key}: ${error instanceof Error ? error.message : String(error)}`
    };
  }
}

/**
 * Validate select/bool parameter (enum validation).
 */
function validateSelect(
  key: string,
  value: string,
  paramDef: ParamDef
): ValidationResult {
  const valueStr = String(value);

  // For bot_on, only accept "0" or "1"
  if (key === 'bot_on') {
    if (valueStr !== '0' && valueStr !== '1') {
      return {
        valid: false,
        error: `${key} must be "0" (Off) or "1" (On), got "${valueStr}"`
      };
    }
    return { valid: true };
  }

  // For other select types, check against options
  if (paramDef.options) {
    const validValues = paramDef.options.map(opt => opt[0]);
    if (!validValues.includes(valueStr)) {
      return {
        valid: false,
        error: `${key} must be one of [${validValues.join(', ')}], got "${valueStr}"`
      };
    }
  }

  return { valid: true };
}

/**
 * Validate integer parameter.
 */
function validateInt(
  key: string,
  value: string,
  paramDef: ParamDef
): ValidationResult {
  const num = Number(value);

  // Check if it's a valid number
  if (isNaN(num) || !isFinite(num)) {
    return {
      valid: false,
      error: `${key} must be a valid number, got "${value}"`
    };
  }

  // Check if it's an integer (no decimals)
  if (!Number.isInteger(num)) {
    return {
      valid: false,
      error: `${key} must be an integer (no decimals), got ${num}`
    };
  }

  // Check min bound
  if (paramDef.min_value !== null && paramDef.min_value !== undefined) {
    if (num < paramDef.min_value) {
      return {
        valid: false,
        error: `${key} must be >= ${paramDef.min_value}, got ${num}`
      };
    }
  }

  // Check max bound
  if (paramDef.max_value !== null && paramDef.max_value !== undefined) {
    if (num > paramDef.max_value) {
      return {
        valid: false,
        error: `${key} must be <= ${paramDef.max_value}, got ${num}`
      };
    }
  }

  return { valid: true };
}

/**
 * Validate float parameter.
 */
function validateFloat(
  key: string,
  value: string,
  paramDef: ParamDef
): ValidationResult {
  const num = Number(value);

  // Check if it's a valid number
  if (isNaN(num) || !isFinite(num)) {
    return {
      valid: false,
      error: `${key} must be a valid number, got "${value}"`
    };
  }

  // Check min bound
  if (paramDef.min_value !== null && paramDef.min_value !== undefined) {
    if (num < paramDef.min_value) {
      const unit = paramDef.unit ? ` ${paramDef.unit}` : '';
      return {
        valid: false,
        error: `${key} must be >= ${paramDef.min_value}${unit}, got ${num}${unit}`
      };
    }
  }

  // Check max bound
  if (paramDef.max_value !== null && paramDef.max_value !== undefined) {
    if (num > paramDef.max_value) {
      const unit = paramDef.unit ? ` ${paramDef.unit}` : '';
      return {
        valid: false,
        error: `${key} must be <= ${paramDef.max_value}${unit}, got ${num}${unit}`
      };
    }
  }

  return { valid: true };
}

/**
 * Validate multiple parameters at once.
 *
 * @param params - Object of parameter key-value pairs
 * @param schema - Full parameter schema
 * @returns Object with valid flag and map of field errors
 */
export function validateParams(
  params: Record<string, any>,
  schema: ParamSchema
): { valid: boolean; errors: ValidationErrors } {
  const errors: ValidationErrors = {};

  for (const [key, value] of Object.entries(params)) {
    const paramDef = schema.params[key];

    if (!paramDef) {
      // Unknown parameter - skip or warn
      if (import.meta.env?.DEV) {
        console.warn(`Unknown parameter: ${key}`);
      }
      continue;
    }

    const result = validateParam(key, value, paramDef);
    if (!result.valid && result.error) {
      errors[key] = result.error;
    }
  }

  return {
    valid: Object.keys(errors).length === 0,
    errors
  };
}

/**
 * Get user-friendly label for a parameter.
 *
 * @param paramDef - Parameter definition
 * @returns Display label with unit (e.g., "qty (base asset)", "cooldown (seconds)")
 */
export function getParamLabel(paramDef: ParamDef): string {
  if (paramDef.unit) {
    return `${paramDef.label} (${paramDef.unit})`;
  }
  return paramDef.label;
}

/**
 * Format parameter value for display.
 *
 * @param value - Raw parameter value (string)
 * @param paramDef - Parameter definition
 * @returns Formatted value for display
 */
export function formatParamValue(value: string, paramDef: ParamDef): string {
  if (paramDef.type === 'bool' || paramDef.type === 'select') {
    // For bool/select, return the label if options exist
    if (paramDef.options) {
      const option = paramDef.options.find(opt => opt[0] === value);
      if (option) {
        return option[1];  // Return label
      }
    }
    return value;
  }

  if (paramDef.type === 'int') {
    return parseInt(value, 10).toString();
  }

  if (paramDef.type === 'float') {
    const num = parseFloat(value);
    // Show up to 4 decimal places, but trim trailing zeros
    return num.toFixed(4).replace(/\.?0+$/, '');
  }

  return value;
}

/**
 * Get short tooltip text for a parameter header.
 *
 * @param paramDef - Parameter definition
 * @returns Short description with unit
 */
export function getParamTooltip(paramDef: ParamDef): string {
  const unit = paramDef.unit ? ` (${paramDef.unit})` : '';
  const description = paramDef.description.trim();

  // Truncate to the first sentence, but avoid breaking on decimals like "0.25"
  // by only treating a period as sentence-ending when followed by whitespace or EOL.
  const sentenceMatch = description.match(/^(.*?)(?:\.(?:\s|$))/);
  const shortDesc = (sentenceMatch ? sentenceMatch[1] : description).trim();

  return `${shortDesc}${unit}`;
}
