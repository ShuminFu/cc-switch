import * as React from "react";
import * as DialogPrimitive from "@radix-ui/react-dialog";
import { cn } from "@/lib/utils";
import {
  DialogLayerContext,
  Z_LAYERS,
  nextLayer,
  resolveLayer,
  type DialogLayer,
} from "./dialogLayer";

const Dialog = DialogPrimitive.Root;

const DialogTrigger = DialogPrimitive.Trigger;

const DialogPortal = DialogPrimitive.Portal;

const DialogClose = DialogPrimitive.Close;

const DialogOverlay = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Overlay>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Overlay> & {
    zIndex?: DialogLayer;
  }
>(({ className, zIndex = "base", ...props }, ref) => (
  <DialogPrimitive.Overlay
    ref={ref}
    className={cn(
      "fixed inset-0 bg-black/50 backdrop-blur-sm data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 motion-reduce:animate-none",
      Z_LAYERS[zIndex],
      className,
    )}
    {...props}
  />
));
DialogOverlay.displayName = DialogPrimitive.Overlay.displayName;

const DialogContent = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Content> & {
    zIndex?: DialogLayer;
    variant?: "default" | "fullscreen";
    overlayClassName?: string;
    /**
     * Allow closing by clicking the overlay. Off by default so forms and
     * destructive confirmations are not lost to a stray click; turn it on for
     * purely informational dialogs (FE-UI-12).
     */
    dismissOnOutsideClick?: boolean;
  }
>(
  (
    {
      className,
      children,
      zIndex,
      variant = "default",
      overlayClassName,
      dismissOnOutsideClick = false,
      onInteractOutside,
      ...props
    },
    ref,
  ) => {
    const layer = resolveLayer(zIndex, React.useContext(DialogLayerContext));

    const variantClass = {
      default:
        "fixed left-1/2 top-1/2 flex flex-col w-full max-w-lg max-h-[90vh] translate-x-[-50%] translate-y-[-50%] border border-border-default bg-background text-foreground shadow-lg duration-200 data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 data-[state=closed]:slide-out-to-left-1/2 data-[state=closed]:slide-out-to-top-[48%] data-[state=open]:slide-in-from-left-1/2 data-[state=open]:slide-in-from-top-[48%] motion-reduce:animate-none sm:rounded-lg",
      fullscreen:
        "fixed inset-0 flex flex-col w-screen h-screen translate-x-0 translate-y-0 bg-background text-foreground p-0 sm:rounded-none shadow-none",
    }[variant];

    return (
      <DialogPortal>
        <DialogOverlay zIndex={layer} className={overlayClassName} />
        <DialogPrimitive.Content
          ref={ref}
          className={cn(variantClass, Z_LAYERS[layer], className)}
          onInteractOutside={(e) => {
            onInteractOutside?.(e);
            // 默认防止点击遮罩层关闭对话框
            if (!dismissOnOutsideClick) e.preventDefault();
          }}
          {...props}
        >
          {/* Dialogs opened from inside this one stack above it. */}
          <DialogLayerContext.Provider value={nextLayer(layer)}>
            {children}
          </DialogLayerContext.Provider>
        </DialogPrimitive.Content>
      </DialogPortal>
    );
  },
);
DialogContent.displayName = DialogPrimitive.Content.displayName;

const DialogHeader = ({
  className,
  ...props
}: React.HTMLAttributes<HTMLDivElement>) => (
  <div
    className={cn(
      "flex flex-col space-y-1.5 text-center sm:text-left px-6 py-5 border-b border-border-default bg-muted/20 flex-shrink-0",
      className,
    )}
    {...props}
  />
);
DialogHeader.displayName = "DialogHeader";

const DialogFooter = ({
  className,
  ...props
}: React.HTMLAttributes<HTMLDivElement>) => (
  <div
    className={cn(
      "flex flex-col-reverse gap-2 sm:flex-row sm:justify-end sm:items-center px-6 py-5 border-t border-border-default bg-muted/20 flex-shrink-0",
      className,
    )}
    {...props}
  />
);
DialogFooter.displayName = "DialogFooter";

const DialogTitle = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Title>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Title>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Title
    ref={ref}
    className={cn(
      "text-lg font-semibold leading-tight tracking-tight",
      className,
    )}
    {...props}
  />
));
DialogTitle.displayName = DialogPrimitive.Title.displayName;

const DialogDescription = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Description>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Description>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Description
    ref={ref}
    className={cn("text-sm text-muted-foreground", className)}
    {...props}
  />
));
DialogDescription.displayName = DialogPrimitive.Description.displayName;

export {
  Dialog,
  DialogTrigger,
  DialogContent,
  DialogHeader,
  DialogFooter,
  DialogTitle,
  DialogDescription,
  DialogClose,
};
