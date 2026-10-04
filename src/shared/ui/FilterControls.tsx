import { forwardRef, type ButtonHTMLAttributes, type HTMLAttributes, type InputHTMLAttributes, type ReactNode } from "react";

const CHIP_BASE = "cursor-pointer whitespace-nowrap rounded-12 border border-border bg-transparent px-2.25 py-0.5 text-11 text-muted transition-all duration-120";
const CHIP_HOVER = "hover:border-white/25! hover:text-foreground!";
const CHIP_ACTIVE = "border-accent! bg-accent/10! text-accent!";
const CHIP_RESET = "border-white/20! text-foreground! hover:border-white/40! disabled:border-border! disabled:text-muted! disabled:hover:border-border! disabled:hover:text-muted! disabled:opacity-55 disabled:cursor-default";

type FilterChipProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  active?: boolean;
  reset?: boolean;
  children?: ReactNode;
};

export const FilterChip = forwardRef<HTMLButtonElement, FilterChipProps>(function FilterChip({ active = false, reset = false, className = "", children, ...rest }, ref) {
  return <button ref={ref} className={`${CHIP_BASE} ${reset ? CHIP_RESET : CHIP_HOVER}${active ? ` ${CHIP_ACTIVE}` : ""}${className ? ` ${className}` : ""}`} {...rest}>{children}</button>;
});

export function FilterBar({ className = "", ...rest }: HTMLAttributes<HTMLDivElement>) {
  return <div className={`flex shrink-0 flex-wrap items-center gap-1 border-b border-border bg-background px-3 py-1${className ? ` ${className}` : ""}`} {...rest} />;
}

export function FilterSeparator({ className = "", ...rest }: HTMLAttributes<HTMLSpanElement>) {
  return <span className={`mx-0.5 h-3.5 w-px shrink-0 bg-border${className ? ` ${className}` : ""}`} {...rest} />;
}

export function FilterLabel({ className = "", ...rest }: HTMLAttributes<HTMLSpanElement>) {
  return <span className={`whitespace-nowrap text-11 text-muted${className ? ` ${className}` : ""}`} {...rest} />;
}

export const FoundrySearch = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(function FoundrySearch({ className = "", ...rest }, ref) {
  return <input ref={ref} className={`box-border w-full rounded-6 border border-border bg-surface px-2 py-1 text-12 text-foreground outline-none focus:border-accent${className ? ` ${className}` : ""}`} {...rest} />;
});

export function EmptyMessage({ className = "", ...rest }: HTMLAttributes<HTMLDivElement>) {
  return <div className={`px-6 py-10 text-center leading-1.6 text-muted${className ? ` ${className}` : ""}`} {...rest} />;
}
