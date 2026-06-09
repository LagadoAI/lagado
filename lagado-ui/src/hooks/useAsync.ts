// Async handler hook
 
import { useEffect, useState } from "react";
 
export function useAsync<T>(
  fn: () => Promise<T>,
  deps: React.DependencyList = []
) {
  const [data, setData] = useState<T | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<Error | null>(null);
 
  useEffect(() => {
    let isMounted = true;
 
    const execute = async () => {
      setLoading(true);
      setError(null);
      try {
        const result = await fn();
        if (isMounted) {
          setData(result);
        }
      } catch (err) {
        if (isMounted) {
          setError(err instanceof Error ? err : new Error(String(err)));
        }
      } finally {
        if (isMounted) {
          setLoading(false);
        }
      }
    };
 
    execute();
 
    return () => {
      isMounted = false;
    };
  }, deps);
 
  return { data, loading, error };
}
