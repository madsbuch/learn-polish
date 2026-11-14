import { Button } from "../design";
import { ArrowDownIcon } from "./icons";

/** Floats at the foot of a chat scrolled up from its end; back to the newest. */
export function JumpToLatest({ onClick }: { onClick: () => void }) {
  return (
    <div className="jump-latest">
      <Button size="sm" icon={<ArrowDownIcon />} onClick={onClick}>
        Latest
      </Button>
    </div>
  );
}
