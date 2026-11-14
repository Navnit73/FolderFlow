import { cva, type VariantProps } from "class-variance-authority";
import type { ButtonHTMLAttributes, Ref } from "react";
import { cn } from "@/lib/cn";

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-1.5 whitespace-nowrap rounded-md text-sm font-medium transition-colors duration-(--ff-duration-base) ease-ff disabled:pointer-events-none disabled:text-fg-disabled [&_svg]:size-4 [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        primary: "bg-accent text-fg-on-accent hover:bg-accent-hover disabled:bg-muted",
        secondary: "border border-border-strong bg-surface text-fg hover:bg-hover active:bg-active",
        ghost: "text-fg-muted hover:bg-hover hover:text-fg active:bg-active",
        danger: "bg-danger-solid text-fg-on-accent hover:bg-danger-solid-hover disabled:bg-muted",
      },
      size: {
        default: "h-control px-3",
        sm: "h-control-sm px-2 text-xs",
        icon: "size-control",
      },
    },
    defaultVariants: { variant: "secondary", size: "default" },
  },
);

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement>, VariantProps<typeof buttonVariants> {
  ref?: Ref<HTMLButtonElement>;
}

export function Button({ className, variant, size, type = "button", ...props }: ButtonProps) {
  return <button type={type} className={cn(buttonVariants({ variant, size }), className)} {...props} />;
}
