import { useState } from "react";
import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, afterEach } from "vitest";
import { ErrorBoundary } from "@/app/ErrorBoundary";
import {
  ProvidersErrorState,
  shouldShowProvidersError,
} from "@/app/ProvidersErrorState";

function Boom({ explode }: { explode: boolean }) {
  if (explode) {
    throw new Error("kaboom");
  }
  return <div>page content</div>;
}

describe("ErrorBoundary", () => {
  const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});

  afterEach(() => {
    consoleError.mockClear();
  });

  it("renders a recovery screen instead of a blank window", () => {
    render(
      <ErrorBoundary>
        <Boom explode />
      </ErrorBoundary>,
    );

    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(screen.getByText("kaboom")).toBeInTheDocument();
    expect(screen.getByText("Try again")).toBeInTheDocument();
    expect(screen.getByText("Copy diagnostics")).toBeInTheDocument();
    expect(screen.getByText("Open config folder")).toBeInTheDocument();
    expect(screen.getByText("Restart")).toBeInTheDocument();
  });

  it("recovers when the reset key changes", () => {
    function Harness() {
      const [page, setPage] = useState("a");
      return (
        <>
          <button onClick={() => setPage("b")}>go-b</button>
          <ErrorBoundary resetKey={page} variant="page">
            <Boom explode={page === "a"} />
          </ErrorBoundary>
        </>
      );
    }

    render(<Harness />);
    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(screen.queryByText("Restart")).not.toBeInTheDocument();

    fireEvent.click(screen.getByText("go-b"));
    expect(screen.getByText("page content")).toBeInTheDocument();
  });
});

describe("ProvidersErrorState", () => {
  it("shows the backend error and retries", () => {
    const onRetry = vi.fn();
    render(
      <ProvidersErrorState error="database is locked" onRetry={onRetry} />,
    );
    expect(screen.getByText("database is locked")).toBeInTheDocument();
    fireEvent.click(screen.getByText("Retry"));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it("only replaces the list when there is no own data to show", () => {
    expect(
      shouldShowProvidersError({
        isError: true,
        data: undefined,
        isPlaceholderData: false,
      }),
    ).toBe(true);
    expect(
      shouldShowProvidersError({
        isError: true,
        data: {},
        isPlaceholderData: true,
      }),
    ).toBe(true);
    expect(
      shouldShowProvidersError({
        isError: true,
        data: {},
        isPlaceholderData: false,
      }),
    ).toBe(false);
    expect(
      shouldShowProvidersError({
        isError: false,
        data: undefined,
        isPlaceholderData: false,
      }),
    ).toBe(false);
  });
});
