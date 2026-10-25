function metrics = quantify_event_responses(epochs_signal, epoch_time_s_relative, fs, params)
% QUANTIFY_EVENT_RESPONSES Quantifies responses for each epoch.
% Args: see original file
% Returns: see original file

    arguments
        epochs_signal (:,:) double {mustBeNumeric, mustBeReal}
        epoch_time_s_relative (:,1) double {mustBeNumeric, mustBeReal}
        fs (1,1) double {mustBeNumeric, mustBeReal, mustBePositive}
        params (1,1) struct 
    end

    expected_param_fields = {'peak_window_start_sec', 'peak_window_end_sec', ...
                             'auc_window_start_sec', 'auc_window_end_sec', ...
                             'duration_threshold_signal', 'duration_min_interval_sec'};
    grabACHAnalysisTools.mustContainFields(params, expected_param_fields); 

    if size(epochs_signal, 1) ~= length(epoch_time_s_relative), error('grabACHAnalysisTools:quantify_event_responses:DimensionMismatch', 'Epoch signal rows must match time vector length.');end
    num_epochs = size(epochs_signal, 2);
    if num_epochs == 0, metrics = struct('peak_value', {}, 'peak_latency_s', {}, 'auc_value', {}, 'response_duration_s', {}); warning('grabACHAnalysisTools:quantify_event_responses:NoEpochs', 'No epochs for quantification.'); return; end
    if params.peak_window_start_sec >= params.peak_window_end_sec, error('grabACHAnalysisTools:quantify_event_responses:InvalidPeakWindow', 'Peak window start must be < end.'); end
    if params.auc_window_start_sec >= params.auc_window_end_sec, error('grabACHAnalysisTools:quantify_event_responses:InvalidAucWindow', 'AUC window start must be < end.'); end
    if params.duration_min_interval_sec < 0, error('grabACHAnalysisTools:quantify_event_responses:InvalidMinInterval', 'Duration min interval cannot be negative.'); end

    metrics = repmat(struct('peak_value', NaN, 'peak_latency_s', NaN, 'auc_value', NaN, 'response_duration_s', NaN), num_epochs, 1);
    peak_idx_logical = epoch_time_s_relative >= params.peak_window_start_sec & epoch_time_s_relative <= params.peak_window_end_sec;
    if ~any(peak_idx_logical), warning('grabACHAnalysisTools:quantify_event_responses:EmptyPeakWindow', 'Peak window empty. Peak metrics will be NaN.'); end
    auc_idx_logical = epoch_time_s_relative >= params.auc_window_start_sec & epoch_time_s_relative <= params.auc_window_end_sec;
    if ~any(auc_idx_logical), warning('grabACHAnalysisTools:quantify_event_responses:EmptyAucWindow', 'AUC window empty. AUC metrics will be NaN.'); end
    duration_eval_idx_logical = epoch_time_s_relative >= 0; 
    if ~any(duration_eval_idx_logical), warning('grabACHAnalysisTools:quantify_event_responses:NoPostEventDataForDuration', 'No post-event data for duration. Duration will be NaN.'); end
    use_bwlabel = false;
    if params.duration_min_interval_sec > 0
        if license('test', 'image_toolbox') && exist('bwlabel', 'file'), use_bwlabel = true;
        else warning('grabACHAnalysisTools:quantify_event_responses:MissingToolbox', 'Image Processing Toolbox (bwlabel) not found. Duration calculated without merging small intervals.'); end
    end

    for i = 1:num_epochs
        current_epoch_signal = epochs_signal(:, i);
        if any(peak_idx_logical)
            signal_in_peak_window = current_epoch_signal(peak_idx_logical); time_in_peak_window = epoch_time_s_relative(peak_idx_logical);
            if ~all(isnan(signal_in_peak_window)), [peak_val, rel_idx] = max(signal_in_peak_window); metrics(i).peak_value = peak_val; metrics(i).peak_latency_s = time_in_peak_window(rel_idx(1)); end
        end
        if any(auc_idx_logical)
            signal_in_auc_window = current_epoch_signal(auc_idx_logical); time_in_auc_window = epoch_time_s_relative(auc_idx_logical);
            valid_auc_points = ~isnan(signal_in_auc_window);
            if sum(valid_auc_points) >= 2, metrics(i).auc_value = trapz(time_in_auc_window(valid_auc_points), signal_in_auc_window(valid_auc_points));
            elseif sum(valid_auc_points) == 1, metrics(i).auc_value = 0; end
        end
        if any(duration_eval_idx_logical)
            signal_for_duration = current_epoch_signal(duration_eval_idx_logical);
            if ~all(isnan(signal_for_duration))
                above_threshold_logical = signal_for_duration > params.duration_threshold_signal;
                if use_bwlabel && params.duration_min_interval_sec > 0
                    min_samples_interval = round(params.duration_min_interval_sec * fs);
                    if min_samples_interval > 1
                        temp_signal = above_threshold_logical; labeled_regions = bwlabel(temp_signal); region_props = regionprops(labeled_regions, 'Area');
                        valid_regions_mask = false(size(above_threshold_logical));
                        for k_r = 1:length(region_props), if region_props(k_r).Area >= min_samples_interval, valid_regions_mask(labeled_regions == k_r) = true; end; end
                        above_threshold_logical = valid_regions_mask;
                    end
                end
                metrics(i).response_duration_s = sum(above_threshold_logical) / fs;
            end
        end
    end
end