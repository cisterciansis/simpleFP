function fig_handle = plot_summary_statistics(event_metrics, plot_params)
% PLOT_SUMMARY_STATISTICS Plots summary stats (boxplots) for event metrics.
% Args: see original file
% Returns: see original file

    arguments
        event_metrics (:,1) struct
        plot_params (1,1) struct 
    end

    expected_plot_param_fields = {'event_label', 'normalization_method_string', 'output_dir', 'save_plots'};
    grabACHAnalysisTools.mustContainFields(plot_params, expected_plot_param_fields);

    fig_handle = [];
    if isempty(event_metrics), disp('Info: No event_metrics for plot_summary_statistics.'); return; end

    available_fields = string(fieldnames(event_metrics));
    metrics_fields_input = grabACHAnalysisTools.get_plot_param(plot_params, 'metrics_to_plot', available_fields);
    metrics_fields_to_plot = string(metrics_fields_input); % Ensure string array

    is_valid_field = ismember(metrics_fields_to_plot, available_fields);
    if ~all(is_valid_field)
        missing=metrics_fields_to_plot(~is_valid_field); 
        warning('grabACHAnalysisTools:plot_summary_statistics:MissingMetrics', 'Metrics not found: %s. Plotting available from request or all if none valid from request.', strjoin(missing,', ')); 
        metrics_fields_to_plot=metrics_fields_to_plot(is_valid_field); % Keep only valid ones from request
        if isempty(metrics_fields_to_plot), metrics_fields_to_plot = available_fields; end % Fallback to all if request is now empty
    end
    if isempty(metrics_fields_to_plot), metrics_fields_to_plot = available_fields; end 
    num_metrics_to_plot = length(metrics_fields_to_plot);
    if num_metrics_to_plot == 0, disp('Info: No metrics to plot. Skipping.'); return; end

    y_prefix = ""; ns_lower=lower(plot_params.normalization_method_string); if contains(ns_lower,"z-score"),y_prefix="Z-Score ";elseif contains(ns_lower,"dff")&&contains(ns_lower,"local"),y_prefix="Local \DeltaF/F ";elseif ~contains(ns_lower,"globally processed signal")&&~isempty(strip(plot_params.normalization_method_string)),y_prefix=plot_params.normalization_method_string+" ";end
    figure_name_str = sprintf('%s: Summary Statistics (%s)', strrep(plot_params.event_label, '_', ' '), plot_params.normalization_method_string);
    
    n_c = min(num_metrics_to_plot,4); n_r = ceil(num_metrics_to_plot/n_c); fig_w=250*n_c; fig_h=300*n_r;
    fig_handle = figure('Name',figure_name_str,'NumberTitle','off','Visible','on');
    fig_pos_default = []; % Will be calculated if not provided
    scr=get(0,'ScreenSize'); fig_pos_default=[(scr(3)-fig_w)/2,(scr(4)-fig_h)/2,fig_w,fig_h];
    fig_pos = grabACHAnalysisTools.get_plot_param(plot_params,'figure_position',fig_pos_default);
    set(fig_handle,'Position',fig_pos);

    p_clr=grabACHAnalysisTools.get_plot_param(plot_params,'point_color',[0.3 0.3 0.3]); 
    p_sz=grabACHAnalysisTools.get_plot_param(plot_params,'point_size',25); 
    p_alpha=grabACHAnalysisTools.get_plot_param(plot_params,'point_alpha',0.5); 
    box_clr=grabACHAnalysisTools.get_plot_param(plot_params,'boxplot_color','b');

    for i = 1:num_metrics_to_plot
        metric_name = metrics_fields_to_plot(i);
        try data = [event_metrics.(metric_name)]'; catch ME_data, warning('grabACHAnalysisTools:plot_summary_statistics:MetricAccessError','Cannot get data for %s. Error: %s. Skipping.',metric_name, ME_data.message); continue; end
        
        subplot(n_r, n_c, i);
        if all(isnan(data))||isempty(data), text(0.5,0.5,sprintf('No data for\n%s',strrep(metric_name,'_',' ')),'HorizontalAlignment','center'); title(strrep(metric_name,'_',' ')); axis off; continue; end
        
        boxplot(data,'Labels',{''},'Colors',box_clr,'Symbol','k+'); hold on;
        jitter_max = 0.15; x_jit = 1+(rand(length(data),1)-0.5)*2*jitter_max;
        scatter(x_jit, data, p_sz, p_clr, 'filled', 'MarkerFaceAlpha', p_alpha, 'MarkerEdgeColor','none'); hold off;
        
        yl_base=strrep(metric_name,'_',' '); if contains(lower(metric_name),["latency","duration"]), yl_final=sprintf('%s%s (s)',y_prefix,yl_base); else yl_final=sprintf('%s%s',y_prefix,yl_base); end; ylabel(yl_final);
        title(strrep(metric_name,'_',' ')); set(gca,'XTickLabel',{strrep(plot_params.event_label,'_',' ')},'XTick',1); grid on; box on;
    end
    if isvalid(fig_handle) && num_metrics_to_plot > 0 
        try sgtitle(fig_handle,figure_name_str,'FontWeight','bold','Interpreter','none');
        catch ME_sgtitle
             warning('grabACHAnalysisTools:plot_summary_statistics:SGTitleError', 'Could not set super title: %s', ME_sgtitle.message);
        end
    end

    if plot_params.save_plots && isvalid(fig_handle) && num_metrics_to_plot > 0
        grabACHAnalysisTools.save_plot_figure(fig_handle, string(plot_params.output_dir), sprintf('%s_summary_statistics', plot_params.event_label), 'png');
    end
end