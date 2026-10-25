function [normalized_epochs, trial_baselines] = ...
    normalize_epochs_trial_by_trial(epochs_signal, epoch_time_s_relative, ...
                                    pre_event_baseline_start_s, pre_event_baseline_end_s, ...
                                    normalization_method)
% NORMALIZE_EPOCHS_TRIAL_BY_TRIAL Normalizes each epoch by its pre-event baseline.
%
% Args:
%   epochs_signal (matrix): (num_samples x num_epochs).
%   epoch_time_s_relative (vector): Time vector for epochs. Column vector.
%   pre_event_baseline_start_s (scalar): Baseline window start (s).
%   pre_event_baseline_end_s (scalar): Baseline window end (s).
%   normalization_method (string): "zscore", "dff_local", "subtract_mean". Default "zscore".
%
% Returns:
%   normalized_epochs (matrix): Normalized epochs.
%   trial_baselines (struct array): .mean, .std for each trial's baseline.

    arguments
        epochs_signal (:,:) double {mustBeNumeric, mustBeReal}
        epoch_time_s_relative (:,1) double {mustBeNumeric, mustBeReal}
        pre_event_baseline_start_s (1,1) double {mustBeNumeric, mustBeReal}
        pre_event_baseline_end_s (1,1) double {mustBeNumeric, mustBeReal}
        normalization_method (1,1) string {mustBeMember(normalization_method, ["zscore", "dff_local", "subtract_mean"])} = "zscore"
    end

    if size(epochs_signal, 1) ~= length(epoch_time_s_relative), error('grabACHAnalysisTools:normalize_epochs_trial_by_trial:DimensionMismatch', 'Epoch signal rows must match time vector length.'); end

    num_epochs = size(epochs_signal, 2);
    normalized_epochs = zeros(size(epochs_signal), 'like', epochs_signal);
    trial_baselines = repmat(struct('mean', NaN, 'std', NaN), num_epochs, 1);

    if pre_event_baseline_start_s >= 0 || pre_event_baseline_end_s >= 0, warning('grabACHAnalysisTools:normalize_epochs_trial_by_trial:BaselineWindowPositive', 'Pre-event baseline window should ideally be negative.'); end
    if pre_event_baseline_start_s >= pre_event_baseline_end_s, error('grabACHAnalysisTools:normalize_epochs_trial_by_trial:InvalidBaselineWindow', 'Baseline start must be < end time.'); end

    baseline_indices = epoch_time_s_relative >= pre_event_baseline_start_s & epoch_time_s_relative <= pre_event_baseline_end_s;

    if sum(baseline_indices) < 2
        warning('grabACHAnalysisTools:normalize_epochs_trial_by_trial:ShortBaseline', 'Baseline < 2 samples. Normalization might be unstable. Returning original epochs.');
        normalized_epochs = epochs_signal; return;
    end
    
    min_std_threshold = 1e-9;

    for i = 1:num_epochs
        current_epoch = epochs_signal(:, i);
        pre_event_data_trial = current_epoch(baseline_indices);
        
        if all(isnan(pre_event_data_trial))
            warning('grabACHAnalysisTools:normalize_epochs_trial_by_trial:NaNBaseline', 'Baseline for trial %d is all NaN. Skipping normalization.', i);
            normalized_epochs(:, i) = current_epoch; continue;
        end
        
        mean_baseline_trial = mean(pre_event_data_trial, 'omitnan');
        trial_baselines(i).mean = mean_baseline_trial;

        switch lower(normalization_method)
            case "zscore"
                std_baseline_trial = std(pre_event_data_trial, 0, 'omitnan');
                trial_baselines(i).std = std_baseline_trial;
                if isnan(std_baseline_trial) || std_baseline_trial < min_std_threshold
                    normalized_epochs(:, i) = current_epoch - mean_baseline_trial;
                    fprintf_info_once(sprintf('TrialNormInfo_ZscoreUnstable_Trial%d',i), 'Info: Trial %d baseline SD is very small/NaN. Z-score unstable. Subtracting mean.\n', i);
                else
                    normalized_epochs(:, i) = (current_epoch - mean_baseline_trial) / std_baseline_trial;
                end
            case "dff_local"
                if isnan(mean_baseline_trial) || abs(mean_baseline_trial) < min_std_threshold
                    normalized_epochs(:, i) = current_epoch - mean_baseline_trial;
                    fprintf_info_once(sprintf('TrialNormInfo_dFFLocalUnstable_Trial%d',i), 'Info: Trial %d local baseline mean (F0_local) is very small/NaN. dF/F_local unstable. Subtracting mean.\n', i);
                else
                    normalized_epochs(:, i) = (current_epoch - mean_baseline_trial) ./ mean_baseline_trial * 100;
                end
            case "subtract_mean"
                normalized_epochs(:, i) = current_epoch - mean_baseline_trial;
        end
    end
end

function fprintf_info_once(messageID, varargin)
    persistent reportedMessages;
    if isempty(reportedMessages)
        reportedMessages = containers.Map();
    end
    if ~isKey(reportedMessages, messageID)
        fprintf(varargin{:});
        reportedMessages(messageID) = true;
    end
end