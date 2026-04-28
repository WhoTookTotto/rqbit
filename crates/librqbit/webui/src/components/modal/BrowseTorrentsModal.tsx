import { useContext, useRef, useState } from "react";
import { CgSearch } from "react-icons/cg";
import {
  ErrorDetails,
  TorrentSearchResult,
  TorrentSearchSource,
} from "../../api-types";
import { APIContext } from "../../context";
import { Button } from "../buttons/Button";
import { UploadButton } from "../buttons/UploadButton";
import { FormInput } from "../forms/FormInput";
import { TabButton, TabList } from "../Tabs";
import { Modal } from "./Modal";
import { ModalBody } from "./ModalBody";
import { ModalFooter } from "./ModalFooter";

const TORRENT_SOURCES: TorrentSearchSource[] = [
  "piratebay",
  "rargb",
  "ettv",
  "zooqle",
  "kickass",
  "torrentproject",
];

export const BrowseTorrentsModal: React.FC<{
  isOpen: boolean;
  onClose: () => void;
}> = ({ isOpen, onClose }) => {
  const API = useContext(APIContext);
  const [source, setSource] = useState<TorrentSearchSource>("piratebay");
  const [query, setQuery] = useState("");
  const [submittedQuery, setSubmittedQuery] = useState("");
  const [page, setPage] = useState(1);
  const [results, setResults] = useState<TorrentSearchResult[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<ErrorDetails | null>(null);
  const [selectedMagnet, setSelectedMagnet] = useState<string | null>(null);
  const searchRequestId = useRef(0);

  const clearSearchState = () => {
    searchRequestId.current += 1;
    setSubmittedQuery("");
    setResults([]);
    setPage(1);
    setError(null);
    setLoading(false);
  };

  const runSearch = async (
    nextSource: TorrentSearchSource,
    nextQuery: string,
    nextPage: number,
  ) => {
    const trimmedQuery = nextQuery.trim();
    if (!trimmedQuery) {
      clearSearchState();
      return;
    }

    const requestId = ++searchRequestId.current;
    setLoading(true);
    setError(null);
    setResults([]);
    setSubmittedQuery(trimmedQuery);
    setPage(nextPage);

    try {
      const response = await API.searchTorrents(nextSource, trimmedQuery, nextPage);
      if (searchRequestId.current === requestId) {
        setResults(response);
      }
    } catch (e) {
      if (searchRequestId.current === requestId) {
        setResults([]);
        setError(e as ErrorDetails);
      }
    } finally {
      if (searchRequestId.current === requestId) {
        setLoading(false);
      }
    }
  };

  const hasSearched = submittedQuery.length > 0;

  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      title="Browse Torrents"
      className="sm:max-w-3xl"
    >
      <ModalBody>
        <div className="flex flex-col gap-4">
          <TabList className="overflow-x-auto">
            {TORRENT_SOURCES.map((tabSource) => (
              <TabButton
                key={tabSource}
                id={tabSource}
                label={tabSource}
                active={source === tabSource}
                onClick={() => {
                  setSource(tabSource);
                  clearSearchState();
                  if (query.trim()) {
                    void runSearch(tabSource, query, 1);
                  }
                }}
              />
            ))}
          </TabList>

          <div className="rounded-xl border border-divider bg-surface p-3">
            <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:gap-4">
              <CgSearch className="w-5 h-5 text-secondary flex-shrink-0" />
              <div className="flex-1">
                <FormInput
                  autoFocus
                  value={query}
                  name="browse-torrents-search"
                  placeholder={`Search ${source}...`}
                  help="Searches the local torrent search proxy at localhost:8080 through rqbit."
                  disabled={loading}
                  onChange={(e) => {
                    setQuery(e.target.value);
                    clearSearchState();
                  }}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") {
                      void runSearch(source, query, 1);
                    }
                  }}
                />
              </div>
              <Button
                variant="primary"
                disabled={loading || !query.trim()}
                onClick={() => {
                  void runSearch(source, query, 1);
                }}
              >
                {loading ? "Searching..." : "Search"}
              </Button>
            </div>
          </div>

          <section className="rounded-xl border border-divider bg-surface overflow-hidden">
            <div className="px-4 py-3 border-b border-divider bg-surface-raised flex items-center justify-between gap-3">
              <div>
                <div className="font-semibold">{source}</div>
                <div className="text-sm text-secondary">
                  {hasSearched
                    ? `Showing page ${page} for \"${submittedQuery}\"`
                    : "Choose a source and enter a search query to begin."}
                </div>
              </div>
              <div className="flex items-center gap-2">
                <Button
                  size="sm"
                  variant="secondary"
                  disabled={loading || page <= 1 || !hasSearched}
                  onClick={() => {
                    void runSearch(source, submittedQuery, page - 1);
                  }}
                >
                  Prev
                </Button>
                <Button
                  size="sm"
                  variant="secondary"
                  disabled={loading || !hasSearched || results.length === 0}
                  onClick={() => {
                    void runSearch(source, submittedQuery, page + 1);
                  }}
                >
                  Next
                </Button>
              </div>
            </div>

            <div className="p-4 flex flex-col gap-3">
              {!hasSearched && (
                <div className="rounded-lg border border-dashed border-divider p-6 text-center text-secondary">
                  Search across the selected source to populate results here.
                </div>
              )}

              {hasSearched && loading && (
                <div className="rounded-lg border border-divider p-6 text-center text-secondary">
                  Searching {source}...
                </div>
              )}

              {hasSearched && !loading && error && (
                <div className="rounded-lg border border-error-bg bg-error-bg/10 p-4 text-error">
                  {typeof error.text === "string"
                    ? error.text
                    : "Search failed."}
                </div>
              )}

              {hasSearched && !loading && !error && results.length === 0 && (
                <div className="rounded-lg border border-divider p-6 text-center text-secondary">
                  No results found.
                </div>
              )}

              {results.map((item) => (
                <div
                  key={`${item.Url}-${item.Magnet}`}
                  className="rounded-lg border border-divider bg-surface-raised p-4 flex flex-col gap-3"
                >
                  <div className="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
                    <div className="font-medium break-words">{item.Name}</div>
                    <div className="text-xs uppercase tracking-wide text-tertiary shrink-0">
                      {source}
                    </div>
                  </div>

                  <div className="grid gap-2 text-sm text-secondary sm:grid-cols-3">
                    <div>
                      <span className="text-tertiary">Size:</span> {item.Size}
                    </div>
                    <div>
                      <span className="text-tertiary">Uploaded:</span> {item.DateUploaded}
                    </div>
                    <div>
                      <span className="text-tertiary">Category:</span> {item.Category}
                    </div>
                    <div>
                      <span className="text-tertiary">Seeders:</span> {item.Seeders}
                    </div>
                    <div>
                      <span className="text-tertiary">Leechers:</span> {item.Leechers}
                    </div>
                    <div>
                      <span className="text-tertiary">Uploader:</span> {item.UploadedBy}
                    </div>
                  </div>

                  <div className="flex flex-wrap items-center gap-3 text-sm">
                    <a
                      href={item.Url}
                      target="_blank"
                      rel="noreferrer"
                      className="text-primary hover:underline"
                    >
                      Open source page
                    </a>
                    <a
                      href={item.Magnet}
                      className="text-primary hover:underline"
                    >
                      Open magnet
                    </a>
                    <UploadButton
                      onClick={() => setSelectedMagnet(item.Magnet)}
                      data={selectedMagnet === item.Magnet ? item.Magnet : null}
                      resetData={() => setSelectedMagnet(null)}
                      className="group text-primary hover:underline border-0 px-0 py-0 bg-transparent"
                    >
                      <span>Add</span>
                    </UploadButton>
                  </div>
                </div>
              ))}
            </div>
          </section>
        </div>
      </ModalBody>

      <ModalFooter>
        <Button variant="cancel" onClick={onClose}>
          Close
        </Button>
      </ModalFooter>
    </Modal>
  );
};