import { useNavigation } from "@/app/navigation";
import { AgentsPanel } from "@/components/agents/AgentsPanel";

/** Agents page. */
export default function AgentsRoute() {
  const { goBack } = useNavigation();
  return <AgentsPanel onOpenChange={goBack} />;
}
