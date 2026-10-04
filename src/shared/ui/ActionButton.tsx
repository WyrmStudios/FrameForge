import type { ComponentPropsWithRef, ReactNode } from "react";

export type ButtonVariant = "secondary" | "danger";

const BTN_BASE =
  "px-3.5 py-1.5 rounded-6 text-12 cursor-pointer transition-all duration-150 shrink-0 whitespace-nowrap";

const VARIANT_CLASS: Record<ButtonVariant, string> = {
  secondary:
    "border border-border bg-surface text-foreground hover:enabled:border-accent hover:enabled:text-accent disabled:opacity-50 disabled:cursor-default",
  danger:
    "border border-danger/40 bg-danger/8 text-danger hover:border-danger hover:bg-danger/15",
};

type Props = ComponentPropsWithRef<"button"> & {
  variant?: ButtonVariant;
  children?: ReactNode;
};

export function ActionButton({ variant = "secondary", className = "", children, ...rest }: Props) {
  return (
    <button className={`${BTN_BASE} ${VARIANT_CLASS[variant]}${className ? ` ${className}` : ""}`} {...rest}>
      {children}
    </button>
  );
}

export function SecondaryButton(props: Omit<Props, "variant">) {
  return <ActionButton {...props} />;
}

export function DangerButton(props: Omit<Props, "variant">) {
  return <ActionButton variant="danger" {...props} />;
}
