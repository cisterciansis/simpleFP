function fig_handle = plot_event_heatmap(epochs_signal, epoch_time_s_relative, plot_params)
% PLOT_EVENT_HEATMAP Plots a heatmap of signal activity across epochs.
% Args: see original file
% Returns: see original file

    arguments
        epochs_signal (:,:) double {mustBeNumeric, mustBeReal}
        epoch_time_s_relative (:,1) double {mustBeNumeric, mustBeReal}
        plot_params (1,1) struct 
    end

    expected_plot_param_fields = {'event_label', 'normalization_method_string', 'output_dir', 'save_plots'};
    grabACHAnalysisTools.mustContainFields(plot_params, expected_plot_param_fields);

    fig_handle = [];
    if isempty(epochs_signal), disp('Info: No epochs for plot_event_heatmap.'); return; end
    if size(epochs_signal, 1) ~= length(epoch_time_s_relative), error('grabACHAnalysisTools:plot_event_heatmap:DimensionMismatch', 'Epoch signal rows must match time vector length.'); end
    num_trials = size(epochs_signal, 2);
    if num_trials == 0, disp('Info: epochs_signal has zero trials. Skipping plot.'); return; end

    heatmap_data = epochs_signal'; 
    sort_method_input = grabACHAnalysisTools.get_plot_param(plot_params, 'sort_trials_by', ""); 
    if ~isempty(sort_method_input) && num_trials > 1 
        try
            if isnumeric(sort_method_input) && isvector(sort_method_input) && length(sort_method_input) == num_trials
                heatmap_data = heatmap_data(sort_method_input, :);
            elseif isstring(sort_method_input) || ischar(sort_method_input)
                sort_method_str = lower(char(sort_method_input));
                default_sort_end_s = epoch_time_s_relative(end)/2; if default_sort_end_s <=0 && length(epoch_time_s_relative)>1, default_sort_end_s = epoch_time_s_relative(end); elseif default_sort_end_s <=0, default_sort_end_s = 0; end 
                sort_window_end_s = grabACHAnalysisTools.get_plot_param(plot_params,'sort_peak_window_end_s', default_sort_end_s);
                peak_win_idx = epoch_time_s_relative >= 0 & epoch_time_s_relative <= max(0, sort_window_end_s ); 

                if any(peak_win_idx)
                    if strcmp(sort_method_str, "peak_amplitude")
                        [peak_values_in_window, ~] = max(epochs_signal(peak_win_idx,:), [], 1, 'omitnan'); % 'omitnan' is fine with this max syntax
                        [~, sort_indices] = sort(peak_values_in_window, 'descend', 'MissingPlacement','last');
                        heatmap_data = heatmap_data(sort_indices, :);
                    elseif strcmp(sort_method_str, "mean_amplitude")
                        mean_vals = mean(epochs_signal(peak_win_idx,:),1,'omitnan');
                        [~,sort_indices] = sort(mean_vals,'descend','MissingPlacement','last');
                        heatmap_data = heatmap_data(sort_indices,:);
                    else, warning('grabACHAnalysisTools:plot_event_heatmap:UnknownSortString', 'Unknown string sort_method "%s". Not sorting.', sort_method_str); end
                else, warning('grabACHAnalysisTools:plot_event_heatmap:SortWindowInvalid', 'Cannot sort trials, sort window invalid or empty.'); end
            else, warning('grabACHAnalysisTools:plot_event_heatmap:InvalidSortParam', 'Invalid sort_trials_by parameter. Not sorting.'); end
        catch ME, warning('grabACHAnalysisTools:plot_event_heatmap:SortError', 'Error during trial sorting: %s. Not sorting.', ME.message); end
    end

    figure_name_str = sprintf('%s: Event-Aligned Heatmap (%s)', strrep(plot_params.event_label, '_', ' '), plot_params.normalization_method_string);
    fig_handle = figure('Name', figure_name_str, 'NumberTitle', 'off', 'Visible', 'on');
    fig_pos = grabACHAnalysisTools.get_plot_param(plot_params, 'figure_position', [200, 200, 800, 600]);
    set(fig_handle, 'Position', fig_pos);
    
    imagesc(epoch_time_s_relative, 1:num_trials, heatmap_data); hold on;
    xline(0, 'w--', 'LineWidth', 1.5, 'Label', 'Event Onset', 'Color', 'white', 'LabelHorizontalAlignment', 'center', 'LabelVerticalAlignment', 'bottom', 'FontWeight', 'bold');
    hold off;
    xlabel(sprintf('Time from %s Onset (s)', strrep(plot_params.event_label, '_', ' '))); ylabel('Event Trial Number'); title(figure_name_str, 'Interpreter', 'none');
    
    custom_cmap = grabACHAnalysisTools.get_plot_param(plot_params, 'colormap_name', 'jet');
    try colormap(gca, custom_cmap); catch, warning('grabACHAnalysisTools:plot_event_heatmap:ColormapError', 'Failed to apply colormap. Using default.'); colormap(gca, 'jet'); end
    
    cb = colorbar; ylabel(cb, plot_params.normalization_method_string, 'Interpreter', 'tex');
    
    color_lims = grabACHAnalysisTools.get_plot_param(plot_params, 'color_limits', []);
    if ~isempty(color_lims) && isnumeric(color_lims) && length(color_lims) == 2 && color_lims(1) < color_lims(2)
        caxis(color_lims);
    else 
        % Corrected min/max usage for caxis
        min_v = min(heatmap_data(:)); % Default behavior handles NaNs appropriately for vector input
        max_v = max(heatmap_data(:));
        if ~isempty(min_v) && ~isempty(max_v) && min_v < max_v % Ensure min_v and max_v are not empty (e.g. all NaNs)
            if min_v < 0 && max_v > 0
                abs_max = max(abs(min_v), abs(max_v));
                caxis([-abs_max, abs_max]);
            else
                caxis([min_v, max_v]);
            end
        % else: do nothing if min_v or max_v is empty, caxis will auto-scale or handle all NaNs
        end
    end
    
    x_lims = grabACHAnalysisTools.get_plot_param(plot_params, 'x_limits', [epoch_time_s_relative(1), epoch_time_s_relative(end)]); xlim(x_lims);
    if num_trials > 0, ylim([0.5, num_trials + 0.5]); end; box on;
    
    if plot_params.save_plots
        grabACHAnalysisTools.save_plot_figure(fig_handle, string(plot_params.output_dir), sprintf('%s_event_heatmap', plot_params.event_label), 'png');
    end
end