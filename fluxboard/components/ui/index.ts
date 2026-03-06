/**
 * UI Components Barrel Export
 *
 * Central export point for all atomic UI components.
 */

// Button components
export { Button } from './button/Button';
export type { ButtonProps } from './button/Button';
export { IconButton } from './button/IconButton';
export type { IconButtonProps } from './button/IconButton';

// Badge components
export { Badge } from './badge';
export type { BadgeProps, BadgeVariant, BadgeSize } from './badge';
export { StatusDot } from './badge';
export type { StatusDotProps, StatusDotState, StatusDotSize } from './badge';
export { default as TagChip } from './badge/TagChip';
export type { TagChipProps } from './badge/TagChip';

// FilterChip component
export { FilterChip } from './filter-chip';
export type { FilterChipProps } from './filter-chip';

// Input components
export { TextInput } from './input/TextInput';
export type { TextInputProps } from './input/TextInput';
export { NumberInput } from './input/NumberInput';
export type { NumberInputProps } from './input/NumberInput';
export { InlineEditCell } from './input/InlineEditCell';
export type { InlineEditCellProps } from './input/InlineEditCell';
export { Checkbox } from './input/Checkbox';
export type { CheckboxProps } from './input/Checkbox';

// Dialog component
export { Dialog, DialogFooter } from './dialog';
export type { DialogProps, DialogFooterProps } from './dialog';

// Popover component
export { Popover, PopoverClose, PopoverContentWrapper } from './popover';
export type { PopoverProps, PopoverContentWrapperProps } from './popover';

// Select component
export { Select } from './select';
export type { SelectProps, SelectOption } from './select';

// Tooltip component
export { Tooltip, TooltipProvider, SimpleTooltip, IconTooltip } from './tooltip';
export type { TooltipProps, SimpleTooltipProps, IconTooltipProps } from './tooltip';

// Tabs component
export { Tabs, TabsRoot, TabsList, TabsTrigger, TabsContent } from './tabs';
export type { TabsProps, Tab } from './tabs';

// Switch component
export { Switch, SwitchGroup } from './switch';
export type { SwitchProps, SwitchGroupProps } from './switch';

// Toast component
export { Toaster, toast } from './toast';
export type { ToasterProps, ToastOptions, ExternalToast } from './toast';

// ScrollArea component
export {
  ScrollArea,
  ScrollAreaViewport,
  ScrollAreaScrollbar,
  ScrollAreaThumb,
  ScrollAreaCorner,
} from './scroll-area';
export type { ScrollAreaProps } from './scroll-area';

// Toolbar component
export { Toolbar } from './toolbar/Toolbar';
export type { ToolbarProps } from './toolbar/Toolbar';

// KBD component
export { KBD } from './kbd/KBD';
export type { KBDProps } from './kbd/KBD';

// Toggle component
export { ToggleGroup } from './toggle/ToggleGroup';
export type { ToggleGroupProps, ToggleGroupOption } from './toggle/ToggleGroup';
