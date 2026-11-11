import { useQuery } from "@tanstack/react-query";
import { Info } from "lucide-react";
import { useState } from "react";
import { getHealth } from "../api/client";
import { isNewer, useReleaseCheck } from "../lib/releaseCheck";
import { timeAgo } from "../lib/time";
import Modal from "./Modal";
import { Button } from "./ui/button";

/**
 * The topbar's about button and the dialog it opens. The dialog shows the
 * server's version and, when the reviewer turns on the release check, the
 * latest release. A red dot on the button marks a newer release.
 */
export default function About() {
  const [open, setOpen] = useState(false);
  const health = useQuery({ queryKey: ["health"], queryFn: getHealth });
  const { check, setEnabled, refresh } = useReleaseCheck();
  const [refreshing, setRefreshing] = useState(false);
  const version = health.data?.version;
  const latest = check.enabled ? check.latest : null;
  const update =
    latest !== null && version !== undefined && isNewer(latest, version);
  const label = update ? "About nit: a new release is out" : "About nit";
  const close = () => {
    setOpen(false);
  };

  return (
    <>
      <Button
        variant="ghost"
        size="icon"
        className="relative"
        aria-label={label}
        title={label}
        onClick={() => {
          setOpen(true);
        }}
      >
        <Info size={15} aria-hidden />
        {update ? <span className="about-dot" /> : null}
      </Button>
      {open ? (
        <Modal
          title="About nit"
          card="about-modal"
          onDismiss={close}
          actions={<Button onClick={close}>Close</Button>}
        >
          <dl className="about-versions">
            <dt>Version</dt>
            <dd>{version ?? "unknown"}</dd>
            {check.enabled ? (
              <>
                <dt>Latest release</dt>
                <dd>
                  {latest === null ? (
                    "not known yet"
                  ) : (
                    <a
                      href={`https://github.com/Telefragged/nit/releases/tag/${latest}`}
                      target="_blank"
                      rel="noreferrer"
                    >
                      {latest}
                    </a>
                  )}
                </dd>
                <dt>Last checked</dt>
                <dd className="about-checked">
                  {check.checkedAt === 0
                    ? "never"
                    : timeAgo(new Date(check.checkedAt).toISOString())}
                  <Button
                    size="sm"
                    disabled={refreshing}
                    onClick={() => {
                      setRefreshing(true);
                      void refresh().finally(() => {
                        setRefreshing(false);
                      });
                    }}
                  >
                    Refresh
                  </Button>
                </dd>
              </>
            ) : null}
          </dl>
          <label className="check-label" title="Checks once an hour.">
            <input
              type="checkbox"
              checked={check.enabled}
              onChange={(e) => {
                setEnabled(e.target.checked);
              }}
            />
            Check GitHub for new releases
          </label>
        </Modal>
      ) : null}
    </>
  );
}
