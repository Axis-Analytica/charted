import type { ReactNode } from "react";
import { Button as AriaButton, type ButtonProps as AriaButtonProps, Tooltip, TooltipTrigger } from "react-aria-components";
import { Kbd } from "./Kbd";

export interface ButtonProps extends AriaButtonProps {
  variant?: "primary" | "secondary" | "ghost" | "danger";
  size?: "sm" | "md" | "lg";
  icon?: ReactNode;
  children?: ReactNode;
}

export function Button({ variant = "secondary", size = "md", icon, children, className, ...rest }: ButtonProps) {
  return (
    <AriaButton {...rest} className={`btn btn-${variant} btn-${size} ${className ?? ""}`}>
      {icon}
      {children && <span className="btn-label">{children}</span>}
    </AriaButton>
  );
}

export interface IconButtonProps extends Omit<AriaButtonProps, "children"> {
  label: string;
  shortcut?: string;
  icon: ReactNode;
  pressed?: boolean;
}

/** An icon-only button with an accessible label and a tooltip showing its shortcut. */
export function IconButton({ label, shortcut, icon, pressed, className, ...rest }: IconButtonProps) {
  return (
    <TooltipTrigger delay={500} closeDelay={0}>
      <AriaButton {...rest} aria-label={label} aria-pressed={pressed} className={`icon-btn ${className ?? ""}`}>
        {icon}
      </AriaButton>
      <Tooltip className="tooltip" offset={6}>
        {label}
        {shortcut && <Kbd keys={shortcut} />}
      </Tooltip>
    </TooltipTrigger>
  );
}
