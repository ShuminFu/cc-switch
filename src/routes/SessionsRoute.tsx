import { useNavigation } from "@/app/navigation";
import { SessionManagerPage } from "@/components/sessions/SessionManagerPage";

/** Session manager for the active app's shared-feature app. */
export default function SessionsRoute() {
  const { featureApp } = useNavigation();
  return <SessionManagerPage key={featureApp} appId={featureApp} />;
}
