import type { InstallScope, InstalledPackageSummary } from "./types";
import type {
  ArchiveInstallCandidate,
  InstallHandoffCandidate,
  UpdateAllPackagesCandidate,
  UpdatePackageCandidate,
  UrlInstallCandidate,
} from "./view-model";
import { escapeHtml, formatLabel } from "./view-utils";

export function archiveConfirmDialog(candidate: ArchiveInstallCandidate | null) {
  if (!candidate) {
    return "";
  }

  return `
    <div class="modal-backdrop" role="presentation">
      <section class="confirm-dialog" role="dialog" aria-modal="true" aria-labelledby="confirm-install-title">
        <div class="confirm-header">
          <div>
            <p class="eyebrow">Confirm install</p>
            <h2 id="confirm-install-title">${escapeHtml(candidate.name)}</h2>
          </div>
          <span>${escapeHtml(formatLabel(candidate.format))}</span>
        </div>
        <dl class="confirm-facts">
          <div><dt>Archive</dt><dd>${escapeHtml(candidate.archiveName)}</dd></div>
          <div><dt>Type</dt><dd>${escapeHtml(candidate.installType.toUpperCase())}</dd></div>
          <div><dt>Version</dt><dd>${escapeHtml(candidate.version)}</dd></div>
          <div><dt>Destination</dt><dd>${installScopeSelect("archive-install-scope", candidate)}</dd></div>
          <div><dt>Integrity</dt><dd>${escapeHtml(candidate.checksum)}</dd></div>
        </dl>
        <div class="confirm-path">${escapeHtml(candidate.archivePath)}</div>
        <div class="confirm-actions">
          <button class="secondary-action" id="cancel-archive-install" type="button">Cancel</button>
          <button class="primary-action" id="confirm-archive-install" type="button">
            <i data-lucide="shield-check" aria-hidden="true"></i>
            Install archive
          </button>
        </div>
      </section>
    </div>
  `;
}

export function urlConfirmDialog(candidate: UrlInstallCandidate | null) {
  if (!candidate) {
    return "";
  }

  return `
    <div class="modal-backdrop" role="presentation">
      <section class="confirm-dialog" role="dialog" aria-modal="true" aria-labelledby="confirm-url-install-title">
        <div class="confirm-header">
          <div>
            <p class="eyebrow">Confirm download</p>
            <h2 id="confirm-url-install-title">${escapeHtml(candidate.name)}</h2>
          </div>
          <span>${escapeHtml(formatLabel(candidate.format))}</span>
        </div>
        <dl class="confirm-facts">
          <div><dt>Version</dt><dd>${escapeHtml(candidate.version)}</dd></div>
          <div><dt>Destination</dt><dd>${installScopeSelect("url-install-scope", candidate)}</dd></div>
          <div><dt>Integrity</dt><dd>${escapeHtml(candidate.checksum)}</dd></div>
          <div><dt>Source</dt><dd>${escapeHtml(candidate.source)}</dd></div>
        </dl>
        <div class="confirm-actions">
          <button class="secondary-action" id="cancel-url-install" type="button">Cancel</button>
          <button class="primary-action" id="confirm-url-install" type="button">
            <i data-lucide="shield-check" aria-hidden="true"></i>
            Download and install
          </button>
        </div>
      </section>
    </div>
  `;
}

export function handoffConfirmDialog(candidate: InstallHandoffCandidate | null) {
  if (!candidate) {
    return "";
  }

  return `
    <div class="modal-backdrop" role="presentation">
      <section class="confirm-dialog" role="dialog" aria-modal="true" aria-labelledby="confirm-handoff-title">
        <div class="confirm-header ${candidate.privileged ? "danger" : ""}">
          <div>
            <p class="eyebrow">Confirm handoff</p>
            <h2 id="confirm-handoff-title">${escapeHtml(candidate.name)}</h2>
          </div>
          <span>${escapeHtml(candidate.statusLabel)}</span>
        </div>
        <p>${escapeHtml(candidate.message)}</p>
        <dl class="confirm-facts">
          <div><dt>Version</dt><dd>${escapeHtml(candidate.version)}</dd></div>
          <div><dt>Vendor</dt><dd>${escapeHtml(candidate.vendor)}</dd></div>
          <div><dt>State</dt><dd>${candidate.privileged ? "apm will open the vendor PKG but will not run it for you" : "apm will open the external installer target"}</dd></div>
        </dl>
        <div class="confirm-path">${escapeHtml(candidate.target)}</div>
        <div class="confirm-actions">
          <button class="secondary-action" id="cancel-install-handoff" type="button">Cancel</button>
          <button class="primary-action" id="confirm-install-handoff" type="button">
            <i data-lucide="external-link" aria-hidden="true"></i>
            ${escapeHtml(candidate.actionLabel)}
          </button>
        </div>
      </section>
    </div>
  `;
}

type ScopedInstallCandidate = {
  format: string;
  destination: string;
  installScope: InstallScope;
};

function installScopeSelect(id: string, candidate: ScopedInstallCandidate) {
  return `
    <select id="${escapeHtml(id)}" aria-label="Install destination">
      ${installScopeOption(candidate, "user")}
      ${installScopeOption(candidate, "system")}
    </select>
  `;
}

function installScopeOption(candidate: ScopedInstallCandidate, scope: InstallScope) {
  return `
        <option value="${scope}"${candidate.installScope === scope ? " selected" : ""}>${escapeHtml(installScopeLabel(candidate, scope))}</option>
      `;
}

function installScopeLabel(candidate: ScopedInstallCandidate, scope: InstallScope) {
  return `${scope === "user" ? "User library" : "System library"} - ${destinationForScope(candidate, scope)}`;
}

function destinationForScope(candidate: ScopedInstallCandidate, scope: InstallScope) {
  switch (candidate.format.toLowerCase()) {
    case "au":
    case "component":
      return scope === "user"
        ? "~/Library/Audio/Plug-Ins/Components/"
        : "/Library/Audio/Plug-Ins/Components/";
    case "vst3":
      return scope === "user"
        ? "~/Library/Audio/Plug-Ins/VST3/"
        : "/Library/Audio/Plug-Ins/VST3/";
    case "app":
    case "standalone":
      return scope === "user" ? "~/Applications/" : "/Applications/";
    default:
      return candidate.destination;
  }
}

export function removeConfirmDialog(packageItem: InstalledPackageSummary | null) {
  if (!packageItem) {
    return "";
  }

  return `
    <div class="modal-backdrop" role="presentation">
      <section class="confirm-dialog" role="dialog" aria-modal="true" aria-labelledby="confirm-remove-title">
        <div class="confirm-header danger">
          <div>
            <p class="eyebrow">Confirm remove</p>
            <h2 id="confirm-remove-title">${escapeHtml(packageItem.slug)}</h2>
          </div>
          <span>${escapeHtml(packageItem.origin)}</span>
        </div>
        <dl class="confirm-facts">
          <div><dt>Version</dt><dd>${escapeHtml(packageItem.version)}</dd></div>
          <div><dt>Vendor</dt><dd>${escapeHtml(packageItem.vendor)}</dd></div>
          <div><dt>Formats</dt><dd>${escapeHtml(packageItem.formats.map((format) => formatLabel(format.format)).join(" + "))}</dd></div>
          <div><dt>State</dt><dd>${packageItem.origin === "apm" ? "Remove tracked bundles and local state" : "External files will not be deleted"}</dd></div>
        </dl>
        <div class="confirm-path">${escapeHtml(packageItem.formats.map((format) => format.path).join("\n"))}</div>
        <div class="confirm-actions">
          <button class="secondary-action" id="cancel-remove-package" type="button">Cancel</button>
          <button class="primary-action danger-action" id="confirm-remove-package" type="button">
            <i data-lucide="trash-2" aria-hidden="true"></i>
            Remove package
          </button>
        </div>
      </section>
    </div>
  `;
}

export function updateConfirmDialog(update: UpdatePackageCandidate | null) {
  if (!update) {
    return "";
  }

  return `
    <div class="modal-backdrop" role="presentation">
      <section class="confirm-dialog" role="dialog" aria-modal="true" aria-labelledby="confirm-update-title">
        <div class="confirm-header">
          <div>
            <p class="eyebrow">Confirm update</p>
            <h2 id="confirm-update-title">${escapeHtml(update.slug)}</h2>
          </div>
          <span>${escapeHtml(update.available_version)}</span>
        </div>
        <dl class="confirm-facts">
          <div><dt>Installed</dt><dd>${escapeHtml(update.installed_version)}</dd></div>
          <div><dt>Available</dt><dd>${escapeHtml(update.available_version)}</dd></div>
          <div><dt>Vendor</dt><dd>${escapeHtml(update.vendor)}</dd></div>
          ${updateFormatFact(update)}
          <div><dt>State</dt><dd>${escapeHtml(updateStateLabel(update))}</dd></div>
        </dl>
        <div class="confirm-actions">
          <button class="secondary-action" id="cancel-update-package" type="button">Cancel</button>
          <button class="primary-action" id="confirm-update-package" type="button" ${canConfirmUpdate(update) ? "" : "disabled"} title="${escapeHtml(canConfirmUpdate(update) ? updateConfirmTitle(update) : "No tracked format is available for update")}">
            <i data-lucide="shield-check" aria-hidden="true"></i>
            Update package
          </button>
        </div>
      </section>
    </div>
  `;
}

export function updateAllConfirmDialog(candidate: UpdateAllPackagesCandidate | null) {
  if (!candidate) {
    return "";
  }

  const count = candidate.updates.length;
  return `
    <div class="modal-backdrop" role="presentation">
      <section class="confirm-dialog" role="dialog" aria-modal="true" aria-labelledby="confirm-update-all-title">
        <div class="confirm-header">
          <div>
            <p class="eyebrow">Confirm updates</p>
            <h2 id="confirm-update-all-title">${count} ready update${count === 1 ? "" : "s"}</h2>
          </div>
          <span>${count} package${count === 1 ? "" : "s"}</span>
        </div>
        <dl class="confirm-facts">
          <div><dt>Packages</dt><dd>${escapeHtml(updateAllPackageList(candidate))}</dd></div>
          <div><dt>Formats</dt><dd>Tracked formats</dd></div>
          <div><dt>State</dt><dd>Stop if any package needs handoff or cannot update</dd></div>
        </dl>
        <div class="confirm-actions">
          <button class="secondary-action" id="cancel-update-all-packages" type="button">Cancel</button>
          <button class="primary-action" id="confirm-update-all-packages" type="button" ${count > 0 ? "" : "disabled"}>
            <i data-lucide="shield-check" aria-hidden="true"></i>
            Update ready
          </button>
        </div>
      </section>
    </div>
  `;
}

function updateAllPackageList(candidate: UpdateAllPackagesCandidate) {
  const visibleUpdates = candidate.updates.slice(0, 5).map(
    (update) =>
      `${update.slug} ${update.installed_version} -> ${update.available_version}`,
  );
  const remainingCount = candidate.updates.length - visibleUpdates.length;
  return remainingCount > 0
    ? `${visibleUpdates.join(", ")} and ${remainingCount} more`
    : visibleUpdates.join(", ");
}

function updateFormatFact(update: UpdatePackageCandidate) {
  return `
    <div>
      <dt>Formats</dt>
      <dd>${escapeHtml(update.formats.length > 0 ? update.formats.map(formatLabel).join(" + ") : "No tracked format")}</dd>
    </div>
  `;
}

function canConfirmUpdate(update: UpdatePackageCandidate) {
  return update.formats.length > 0;
}

function updateStateLabel(update: UpdatePackageCandidate) {
  if (!canConfirmUpdate(update)) {
    return "No tracked format is available for update";
  }
  return update.formats.length > 1
    ? "Update all tracked formats together"
    : "Attempt managed update and stop for vendor handoff";
}

function updateConfirmTitle(update: UpdatePackageCandidate) {
  return update.formats.length > 1 ? "Update all tracked formats" : "Run managed update";
}
