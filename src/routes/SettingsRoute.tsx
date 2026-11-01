import { useQueryClient } from "@tanstack/react-query";
import { useNavigation } from "@/app/navigation";
import { SettingsPage } from "@/components/settings/SettingsPage";
import { providersApi } from "@/lib/api";
import { queryKeys } from "@/lib/query/keys";

/** Settings page; the initial tab comes from the route parameters. */
export default function SettingsRoute() {
  const { params, goBack } = useNavigation();
  const queryClient = useQueryClient();

  const handleImportSuccess = async () => {
    try {
      // Refetch every app's provider list (active or not) after an import.
      await queryClient.refetchQueries({
        queryKey: queryKeys.providers.all,
        type: "all",
      });
    } catch (error) {
      console.error("[App] Failed to refresh providers after import", error);
    }
    try {
      await providersApi.updateTrayMenu();
    } catch (error) {
      console.error("[App] Failed to refresh tray menu", error);
    }
  };

  return (
    <SettingsPage
      open={true}
      onOpenChange={goBack}
      onImportSuccess={handleImportSuccess}
      defaultTab={params?.tab ?? "general"}
    />
  );
}
