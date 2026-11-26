import DocumentHistory from "./DocumentHistory";
import SendBar from "./SendBar";

const bar = (
  sent: boolean,
  failure: string | null = null,
  shown: string | null = null,
) => (
  // The transform makes this box the containing block of the fixed bar.
  <div
    className="document-page"
    style={{ width: 720, height: 46, transform: "translate(0)" }}
  >
    <SendBar
      sent={sent}
      sending={false}
      failure={failure}
      onSend={() => undefined}
      history={
        <DocumentHistory shown={shown} onLive={() => undefined}>
          {null}
        </DocumentHistory>
      }
      live={shown === null}
    />
  </div>
);

export const Unsent = () => bar(false);

export const Sent = () => bar(true);

export const Failed = () => bar(false, "document 1 not found");

export const VersionOnScreen = () => bar(false, null, "Version 2");
